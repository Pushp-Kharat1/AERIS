use aeris_core::{Fixed8, Symbol, TimestampNs};
use memmap2::{MmapMut, MmapOptions};
use std::fs::OpenOptions;
use std::path::Path;
use std::sync::atomic::{fence, Ordering};

pub const MAGIC: u32 = 0x41455249; // 'AERI'

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MacroPayload {
    pub updated_at_ns: TimestampNs,
    pub macro_bias: u8,
    pub leverage_cap_fixed8: Fixed8,
    pub universe_len: u8,
    pub universe: [Symbol; 8],
}

#[repr(C)]
pub struct PingPongLayout {
    pub magic: u32,
    pub active_index: u32,
    pub sequence: u64,
    pub buffer_0: MacroPayload,
    pub buffer_1: MacroPayload,
}

pub struct PingPongSharedMemory {
    mmap: MmapMut,
}

impl PingPongSharedMemory {
    pub fn create_or_open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let layout_size = std::mem::size_of::<PingPongLayout>() as u64;
        let file_len = file.metadata()?.len();
        if file_len < layout_size {
            file.set_len(layout_size.max(4096))?;
        }

        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        let layout = unsafe { &mut *(mmap.as_mut_ptr() as *mut PingPongLayout) };
        if layout.magic != MAGIC {
            layout.magic = MAGIC;
            layout.active_index = 0;
            layout.sequence = 0;
            layout.buffer_0 = MacroPayload::default();
            layout.buffer_1 = MacroPayload::default();
        }

        Ok(Self { mmap })
    }

    #[inline(always)]
    fn layout(&self) -> &PingPongLayout {
        unsafe { &*(self.mmap.as_ptr() as *const PingPongLayout) }
    }

    #[inline(always)]
    fn layout_mut(&mut self) -> &mut PingPongLayout {
        unsafe { &mut *(self.mmap.as_mut_ptr() as *mut PingPongLayout) }
    }

    #[inline(always)]
    pub fn read_active(&self) -> (u64, MacroPayload) {
        let layout = self.layout();
        let idx = layout.active_index & 1;
        fence(Ordering::Acquire);
        let payload = if idx == 0 {
            layout.buffer_0
        } else {
            layout.buffer_1
        };
        (layout.sequence, payload)
    }

    #[inline(always)]
    pub fn stage_and_flip(&mut self, payload: &MacroPayload) -> u64 {
        let layout = self.layout_mut();
        let next_idx = 1 - (layout.active_index & 1);

        if next_idx == 0 {
            layout.buffer_0 = *payload;
        } else {
            layout.buffer_1 = *payload;
        }

        fence(Ordering::Release);
        layout.active_index = next_idx;
        layout.sequence += 1;
        layout.sequence
    }
}
