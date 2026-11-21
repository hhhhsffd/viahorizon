// SPDX-License-Identifier: GPL-3.0-or-later
//! Keeps translation between a private in-process pipe and the server connection.
use super::{vfp_core as core, vfp_session};
use horizon_client::{auth::AuthSession, net::Cookies};
use horizon_protocol::{
    connection::{Closer, ConnReader, ConnWriter, Connection, Pipe},
    packets::{Empty, handshake, login, status},
};
use std::{
    sync::{Arc, OnceLock, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Packet = (i32, Vec<u8>);
pub type Capabilities = Arc<OnceLock<Arc<vfp_session::PlayCapabilities>>>;

pub fn start(
    address: String,
    auth: Option<Arc<AuthSession>>,
    cookies: Option<Cookies>,
    config: core::Config,
) -> core::Result<(Pipe, Capabilities)> {
    let (client, peer) = Pipe::pair();
    let caps = Arc::new(OnceLock::new());
    let worker_caps = caps.clone();
    thread::Builder::new()
        .name("viahorizon-translation".into())
        .spawn(move || {
            if let Err(error) = run(
                Connection::local(peer),
                &address,
                auth,
                cookies,
                config,
                worker_caps,
            ) {
                use std::io::Write as _;
                let _ = writeln!(std::io::stderr().lock(), "ViaHorizon: {error}");
            }
        })
        .map_err(|error| error.to_string())?;
    Ok((client, caps))
}

enum Translation {
    Native,
    Legacy(Box<vfp_session::Session>),
}

impl Translation {
    fn send(&mut self, id: i32, data: &[u8], remote: &mut ConnWriter) -> core::Result<()> {
        let body = body(id, data);
        match self {
            Self::Native => remote.send(&body).map_err(message),
            Self::Legacy(session) => {
                for translated in session.serverbound(&body)? {
                    remote.send(&translated).map_err(message)?;
                }
                Ok(())
            }
        }
    }

    fn ingest(&mut self, (id, data): Packet) -> core::Result<Option<Packet>> {
        match self {
            Self::Native => Ok(Some((id, data))),
            Self::Legacy(session) => session.ingest(id, &data),
        }
    }

    fn drain(&mut self, local: &mut ConnWriter) -> core::Result<()> {
        let Self::Legacy(session) = self else {
            return Ok(());
        };
        while let Some(packet) = session.poll() {
            forward(packet.map_err(message)?, local)?;
        }
        Ok(())
    }
}

enum Event {
    Local(horizon_protocol::Result<Packet>),
    Remote(horizon_protocol::Result<Packet>),
}

fn reader(
    mut reader: ConnReader,
    send: mpsc::SyncSender<Event>,
    event: fn(horizon_protocol::Result<Packet>) -> Event,
) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("viahorizon-reader".into())
        .spawn(move || {
            loop {
                let packet = reader.recv();
                let failed = packet.is_err();
                if send.send(event(packet)).is_err() || failed {
                    break;
                }
            }
        })
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn body(id: i32, data: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(data.len() + 5);
    core::put_varint(&mut body, id);
    body.extend_from_slice(data);
    body
}

fn forward((id, data): Packet, local: &mut ConnWriter) -> core::Result<()> {
    local.send(&body(id, &data)).map_err(message)
}

struct Close([Closer; 2]);

impl Drop for Close {
    fn drop(&mut self) {
        for closer in &self.0 {
            closer.close();
        }
    }
}

fn run(
    mut local: Connection,
    address: &str,
    auth: Option<Arc<AuthSession>>,
    cookies: Option<Cookies>,
    config: core::Config,
    caps: Capabilities,
) -> core::Result<()> {
    let (host, port) = core::address(address)?;
    let selection = config.selection(&host, port);
    let detected = if selection == core::Selection::Auto {
        Some(probe_version(&host, port)?)
    } else {
        None
    };
    let target = core::choose(&selection, detected, config.login_preview)?;
    let mut state = if let Some(target) = target {
        if !config.login_preview
            && let Some(account) = &auth
            && let Ok(identity) = super::vfp_certificates::get(account)
        {
            super::vfp_chat_signing::prepare(Some(identity));
        }
        let session = if config.login_preview {
            vfp_session::Session::preview(target)?
        } else {
            vfp_session::Session::gameplay(target)?
        };
        let _ = caps.set(session.capabilities());
        Translation::Legacy(Box::new(session))
    } else {
        Translation::Native
    };
    let (mut remote, resolved_host, resolved_port) =
        Connection::connect_server_with_address(&host, port, Duration::from_secs(15))
            .map_err(message)?;
    let close = Close([
        local.reader.closer().map_err(message)?,
        remote.reader.closer().map_err(message)?,
    ]);
    local
        .reader
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(message)?;
    let (id, data) = local.recv().map_err(message)?;
    let handshake::Serverbound::Intention(mut intention) =
        handshake::Serverbound::decode(id, &data).map_err(message)?
    else {
        return Err("Missing local handshake".into());
    };
    intention.host = resolved_host;
    intention.port = resolved_port;
    if let Some(target) = target {
        intention.protocol = target.protocol;
    }
    if cookies.is_some() && intention.protocol >= 766 {
        intention.intent = handshake::TRANSFER;
    }
    remote
        .send(&handshake::Serverbound::Intention(intention).encode())
        .map_err(message)?;
    let (id, data) = local.recv().map_err(message)?;
    state.send(id, &data, &mut remote.writer)?;

    // Keep the remote reader and writer together until login's compression
    // and cipher negotiations finish. The client-facing pipe is uncompressed.
    remote
        .reader
        .set_read_timeout(Some(Duration::from_millis(20)))
        .map_err(message)?;
    local.reader.set_read_timeout(None).map_err(message)?;
    let (send, receive) = mpsc::sync_channel(8);
    let (local_reader, mut local_writer) = local.split();
    let local_thread = reader(local_reader, send.clone(), Event::Local).map_err(message)?;
    let result = (|| {
        let started = Instant::now();
        loop {
            while let Ok(Event::Local(packet)) = receive.try_recv() {
                let (id, data) = packet.map_err(message)?;
                state.send(id, &data, &mut remote.writer)?;
            }
            let packet = match remote.recv() {
                Ok(packet) => packet,
                Err(error)
                    if error.is_timed_out() && started.elapsed() < Duration::from_secs(30) =>
                {
                    continue;
                }
                Err(error) => return Err(message(error)),
            };
            let Some((id, data)) = state.ingest(packet)? else {
                continue;
            };
            match login::Clientbound::decode(id, &data).map_err(message)? {
                login::Clientbound::Compression(compression) => {
                    remote.set_compression(compression.threshold)
                }
                login::Clientbound::Hello(hello) => {
                    authorize(&mut remote, &mut state, hello, auth.as_deref())?
                }
                login::Clientbound::CookieRequest(query) if cookies.is_some() => {
                    let payload = cookies.as_ref().and_then(|jar| {
                        jar.lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .get(&query.key)
                            .cloned()
                    });
                    let packet = login::Serverbound::CookieResponse(login::CookieResponse {
                        key: query.key,
                        payload,
                    })
                    .encode();
                    let (id, data) = split_body(&packet)?;
                    state.send(id, data, &mut remote.writer)?;
                }
                login::Clientbound::LoginFinished(_) => {
                    forward((id, data), &mut local_writer)?;
                    break;
                }
                _ => forward((id, data), &mut local_writer)?,
            }
        }
        remote.reader.set_read_timeout(None).map_err(message)?;
        let (remote_reader, mut remote_writer) = remote.split();
        let remote_thread = reader(remote_reader, send, Event::Remote).map_err(message)?;
        let result = play(&receive, &mut state, &mut local_writer, &mut remote_writer);
        drop(close);
        // Reader threads can be blocked either on the socket or on the queue.
        // Close both before joining them.
        drop(receive);
        let _ = remote_thread.join();
        result
    })();
    let _ = local_thread.join();
    result
}

fn split_body(packet: &[u8]) -> core::Result<(i32, &[u8])> {
    let mut reader = horizon_protocol::buf::Reader::new(packet);
    let id = reader.varint().map_err(message)?;
    Ok((id, &packet[reader.position()..]))
}

fn authorize(
    remote: &mut Connection,
    state: &mut Translation,
    hello: login::Hello,
    auth: Option<&AuthSession>,
) -> core::Result<()> {
    use horizon_protocol::{crypto, rsa::RsaPublicKey};
    let key = RsaPublicKey::from_der(&hello.public_key).map_err(message)?;
    let secret = crypto::generate_secret().map_err(message)?;
    if hello.should_authenticate {
        let account = auth.ok_or("Sign in through Horizon before joining an online server")?;
        let hash = crypto::server_hash(&hello.server_id, &secret, &hello.public_key);
        horizon_client::auth::join_server(account, &hash)
            .map_err(|_| "Horizon could not authenticate with the server")?;
    }
    let packet = login::Serverbound::Key(login::Key {
        key: key.encrypt(&secret).map_err(message)?,
        challenge: key.encrypt(&hello.challenge).map_err(message)?,
    })
    .encode();
    let (id, data) = split_body(&packet)?;
    state.send(id, data, &mut remote.writer)?;
    remote.enable_encryption(&secret);
    Ok(())
}

fn play(
    receive: &mpsc::Receiver<Event>,
    state: &mut Translation,
    local: &mut ConnWriter,
    remote: &mut ConnWriter,
) -> core::Result<()> {
    loop {
        match receive.recv_timeout(Duration::from_millis(10)) {
            Ok(Event::Local(packet)) => {
                let (id, data) = packet.map_err(message)?;
                state.send(id, &data, remote)?;
            }
            Ok(Event::Remote(packet)) => {
                if let Some(packet) = state.ingest(packet.map_err(message)?)? {
                    forward(packet, local)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
        state.drain(local)?;
    }
}

fn probe_version(host: &str, port: u16) -> core::Result<i32> {
    let timeout = Duration::from_secs(3);
    let mut connection = Connection::connect_server(host, port, timeout).map_err(message)?;
    connection
        .reader
        .set_read_timeout(Some(timeout))
        .map_err(message)?;
    connection
        .send(
            &handshake::Serverbound::Intention(handshake::Intention {
                protocol: core::CLIENT_PROTOCOL,
                host: host.into(),
                port,
                intent: handshake::STATUS,
            })
            .encode(),
        )
        .map_err(message)?;
    connection
        .send(&status::Serverbound::StatusRequest(Empty).encode())
        .map_err(message)?;
    let (id, data) = connection.recv().map_err(message)?;
    match status::Clientbound::decode(id, &data).map_err(message)? {
        status::Clientbound::StatusResponse(response) => Ok(core::status_json(&response.json)?.1),
        _ => Err("Unexpected server status packet".into()),
    }
}
