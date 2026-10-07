//! An offscreen render target read back as BGRA bytes (the format of
//! GPUI's sprite atlas).
//!
//! GPUI (gpui-pre 0.3.8) has no public way to paint a texture of ours (see
//! NOTES-visuals.md), so a frame crosses through memory: render, copy to a
//! mapped buffer, hand the app the bytes. Two slots are in flight: a frame
//! shows one frame after it was rendered, and the CPU never waits for the
//! GPU.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};

use crate::gpu::{FORMAT, Gpu, extent};

/// One rendered frame, BGRA, `width * height * 4` bytes.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub cost: FrameCost,
}

/// Where the time of one frame went, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameCost {
    /// Encoding and submitting the render and the copy.
    pub submit: f32,
    /// Waiting for the previous frame's buffer to map.
    pub wait: f32,
    /// Copying the mapped bytes out.
    pub copy: f32,
}

type MapResult = mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>;

struct Slot {
    texture: wgpu::Texture,
    buffer: wgpu::Buffer,
    /// Set while a copy into `buffer` is in flight; receives the map result.
    pending: Option<(wgpu::SubmissionIndex, MapResult)>,
}

pub(crate) struct Target {
    width: u32,
    height: u32,
    padded_row: u32,
    slots: [Slot; 2],
    next: usize,
}

impl Target {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let padded_row = padded_row(width);
        Self {
            width,
            height,
            padded_row,
            slots: [0, 1].map(|_| slot(gpu, width, height, padded_row)),
            next: 0,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Draws `width`×`height` frames from now on. Frames in flight are
    /// dropped, so the next [`Self::frame`] shows nothing yet.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        if (width, height) != (self.width, self.height) {
            *self = Self::new(gpu, width, height);
        }
    }

    /// Forgets the frame in flight: the next [`Self::frame`] shows nothing.
    pub fn discard(&mut self) {
        for slot in &mut self.slots {
            if slot.pending.take().is_some() {
                slot.buffer.unmap();
            }
        }
    }

    /// Renders a frame with `draw` (after the pipeline and bind group are
    /// set by it) and returns the one rendered on the previous call.
    pub fn frame(
        &mut self,
        gpu: &Gpu,
        draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
    ) -> Result<Option<Frame>> {
        let mut cost = FrameCost::default();
        let started = Instant::now();
        let current = self.next;
        let previous = 1 - current;
        self.next = previous;
        self.submit(gpu, current, draw);
        cost.submit = ms(started);

        let Some((index, done)) = self.slots[previous].pending.take() else {
            return Ok(None);
        };
        let waited = Instant::now();
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(Duration::from_secs(2)),
            })
            .map_err(|e| anyhow!("waiting for the visuals frame: {e}"))?;
        done.recv()
            .map_err(|_| anyhow!("map callback dropped"))?
            .map_err(|e| anyhow!("mapping the visuals frame: {e}"))?;
        cost.wait = ms(waited);

        let copied = Instant::now();
        let bgra = self.take_bytes(previous);
        cost.copy = ms(copied);
        Ok(Some(Frame {
            width: self.width,
            height: self.height,
            bgra,
            cost,
        }))
    }

    /// Renders a frame with `draw` and waits for it: for a still picture
    /// that should show at once rather than a frame later. The frame in
    /// flight, if any, is dropped.
    pub fn frame_now(
        &mut self,
        gpu: &Gpu,
        draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
    ) -> Result<Frame> {
        self.discard();
        let slot = self.next;
        self.next = 1 - slot;
        let started = Instant::now();
        self.submit(gpu, slot, draw);
        let mut cost = FrameCost {
            submit: ms(started),
            ..FrameCost::default()
        };
        let (index, done) = self.slots[slot]
            .pending
            .take()
            .ok_or_else(|| anyhow!("no frame in flight"))?;
        let waited = Instant::now();
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(Duration::from_secs(2)),
            })
            .map_err(|e| anyhow!("waiting for the visuals frame: {e}"))?;
        done.recv()
            .map_err(|_| anyhow!("map callback dropped"))?
            .map_err(|e| anyhow!("mapping the visuals frame: {e}"))?;
        cost.wait = ms(waited);
        let copied = Instant::now();
        let bgra = self.take_bytes(slot);
        cost.copy = ms(copied);
        Ok(Frame {
            width: self.width,
            height: self.height,
            bgra,
            cost,
        })
    }

    fn submit(&mut self, gpu: &Gpu, slot: usize, draw: impl FnOnce(&mut wgpu::RenderPass<'_>)) {
        let view = self.slots[slot]
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw(&mut pass);
        }
        let target = &self.slots[slot];
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &target.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            extent(self.width, self.height),
        );
        let index = gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        target
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        self.slots[slot].pending = Some((index, rx));
    }

    fn take_bytes(&self, slot: usize) -> Vec<u8> {
        let buffer = &self.slots[slot].buffer;
        let row = (self.width * 4) as usize;
        let mut bytes = Vec::with_capacity(row * self.height as usize);
        {
            let mapped = buffer.slice(..).get_mapped_range();
            for line in mapped.chunks(self.padded_row as usize) {
                bytes.extend_from_slice(&line[..row]);
            }
        }
        buffer.unmap();
        bytes
    }
}

fn ms(since: Instant) -> f32 {
    since.elapsed().as_secs_f32() * 1000.0
}

fn padded_row(width: u32) -> u32 {
    (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
}

fn slot(gpu: &Gpu, width: u32, height: u32, padded_row: u32) -> Slot {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("visuals frame"),
        size: extent(width, height),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("visuals readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    Slot {
        texture,
        buffer,
        pending: None,
    }
}
