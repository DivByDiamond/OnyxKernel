//! Block-stream reader/writer over the packed snapshot payload.
use super::format::{Area, Block, Io, checksum};
use onyx_core::errno::{Errno, KResult};
use onyx_core::formats::ONYFS_BLOCK_SIZE;

pub(super) struct Stream<'a, I> {
    pub io: &'a mut I,
    pub area: Area,
    pub at: u32,
    buffer: Block,
    offset: usize,
    loaded: bool,
    pub bytes: u32,
    pub hash: u32,
}
impl<'a, I: Io> Stream<'a, I> {
    pub fn new(io: &'a mut I, area: Area, at: u32) -> Self {
        Self {
            io,
            area,
            at,
            buffer: [0; ONYFS_BLOCK_SIZE],
            offset: 0,
            loaded: false,
            bytes: 0,
            hash: 2166136261,
        }
    }
    pub fn write(&mut self, mut input: &[u8]) -> KResult<()> {
        while !input.is_empty() {
            if self.offset == ONYFS_BLOCK_SIZE {
                self.io.write(self.at, &self.buffer)?;
                self.at = self.area.next(self.at)?;
                self.offset = 0;
                self.buffer.fill(0);
            }
            let n = input.len().min(ONYFS_BLOCK_SIZE - self.offset);
            self.buffer[self.offset..self.offset + n].copy_from_slice(&input[..n]);
            self.hash = checksum(self.hash, &input[..n]);
            self.bytes += n as u32;
            self.offset += n;
            input = &input[n..];
        }
        Ok(())
    }
    pub fn finish(&mut self) -> KResult<()> {
        self.io.write(self.at, &self.buffer)
    }
    pub fn read(&mut self, mut output: &mut [u8]) -> KResult<()> {
        while !output.is_empty() {
            if self.offset == ONYFS_BLOCK_SIZE {
                self.at = self.area.next(self.at).map_err(|_| Errno::Io)?;
                self.offset = 0;
                self.loaded = false;
            }
            if !self.loaded {
                self.io.read(self.at, &mut self.buffer)?;
                self.loaded = true;
            }
            let n = output.len().min(ONYFS_BLOCK_SIZE - self.offset);
            output[..n].copy_from_slice(&self.buffer[self.offset..self.offset + n]);
            self.hash = checksum(self.hash, &output[..n]);
            self.bytes += n as u32;
            self.offset += n;
            output = &mut output[n..];
        }
        Ok(())
    }
}
