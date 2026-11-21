// SPDX-License-Identifier: GPL-3.0-or-later
// Adapters for the public SDK. Reader and Component no longer expose the
// private operations used by the 0.8 source mixins.
pub mod buf {
    use horizon_core::nbt::{self, Tag};
    pub use horizon_protocol::buf::*;
    use horizon_protocol::{Error, Result};

    pub struct Reader<'a> {
        inner: horizon_protocol::buf::Reader<'a>,
        data: &'a [u8],
    }

    impl<'a> Reader<'a> {
        pub fn new(data: &'a [u8]) -> Self {
            Self {
                inner: horizon_protocol::buf::Reader::new(data),
                data,
            }
        }

        pub fn unread(&self) -> &'a [u8] {
            &self.data[self.inner.position()..]
        }

        pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
            self.inner.bytes(n)
        }

        pub fn count(&mut self, max: usize) -> Result<usize> {
            self.inner.count(max)
        }
    }

    impl<'a> std::ops::Deref for Reader<'a> {
        type Target = horizon_protocol::buf::Reader<'a>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    impl std::ops::DerefMut for Reader<'_> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner
        }
    }

    pub trait NamedReader {
        fn vfp_named_nbt(&mut self) -> Result<Tag>;
    }

    pub trait NamedWriter {
        fn vfp_named_nbt(&mut self, tag: &Tag) -> Result<()>;
    }

    impl NamedReader for Reader<'_> {
        fn vfp_named_nbt(&mut self) -> Result<Tag> {
            let (root, used) = nbt::read_named(self.unread(), nbt::Limits::network())
                .map_err(|error| Error::new(format!("Legacy NBT: {error}")))?;
            self.bytes(used)?;
            Ok(root.tag)
        }
    }

    impl NamedWriter for Writer {
        fn vfp_named_nbt(&mut self, tag: &Tag) -> Result<()> {
            let root = nbt::NamedTag {
                name: Default::default(),
                tag: tag.clone(),
            };
            let bytes = nbt::write_named(&root, nbt::Limits::network())
                .map_err(|error| Error::new(format!("Legacy NBT: {error}")))?;
            self.bytes(&bytes);
            Ok(())
        }
    }
}

pub mod item_compat {
    use super::buf::{Codec, Reader, Writer};
    use horizon_protocol::{
        Error, Result,
        item::{Component, Slot, Stack},
    };

    /// Slot's public codec can decode a single component inside a wrapper.
    pub fn read(kind: i32, reader: &mut Reader<'_>) -> Result<Component> {
        let mut wrapper = Writer::new();
        for value in [1, 0, 1, 0, kind] {
            wrapper.varint(value);
        }
        let prefix = wrapper.data.len();
        wrapper.bytes(reader.unread());
        let mut wrapped = horizon_protocol::buf::Reader::new(&wrapper.data);
        let mut stack =
            Slot::read(&mut wrapped)?.ok_or_else(|| Error::new("Missing component wrapper"))?;
        if stack.added.len() != 1 || stack.added[0].0 != kind {
            return Err(Error::new("Invalid component wrapper"));
        }
        reader.bytes(wrapped.position() - prefix)?;
        Ok(stack.added.remove(0).1)
    }

    pub fn write(value: &Component, kind: i32, writer: &mut Writer) {
        let mut stack = Stack::new(0, 1);
        stack.added.push((kind, value.clone()));
        let mut wrapper = Writer::new();
        Slot::write(&Some(stack), &mut wrapper);
        let mut payload = Reader::new(&wrapper.data);
        for _ in 0..5 {
            payload.varint().expect("SDK component wrapper header");
        }
        writer.bytes(payload.rest());
    }

    pub trait ComponentWrite {
        fn vfp_write(&self, kind: i32, writer: &mut Writer);
    }

    impl ComponentWrite for Component {
        fn vfp_write(&self, kind: i32, writer: &mut Writer) {
            write(self, kind, writer);
        }
    }
}
