//! The Enhanced tier's shadow map (docs/beryl-3d.md, "The Enhanced tier": "A shadow map"),
//! sampled by `lit_by_key` in `shadow.wgsl`.
//!
//! Not drawn yet: the map is one texel at the far plane, so every fragment is lit by the key
//! light, and the light's view and projection is the identity.

use beryl_scene::frame::Frame;
use glam::Mat4;

use crate::draw::Draws;
use crate::pipeline::DEPTH;

/// The shadow map and what draws it.
pub(crate) struct Shadow {
    /// The map, bound as bind group 1's binding 1.
    pub(crate) view: wgpu::TextureView,
}

impl Shadow {
    /// A shadow map of one texel cleared to the far plane, 1.0.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow map"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear shadow map"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        queue.submit([encoder.finish()]);
        Self { view }
    }

    /// Records the shadow map's pass for the frame `draws` describes into `encoder`, and returns
    /// how many draw calls it took. It records nothing yet.
    #[expect(
        clippy::unused_self,
        reason = "the shadow map is not drawn yet; drawing it reads the map's own state"
    )]
    pub(crate) fn draw(&mut self, _encoder: &mut wgpu::CommandEncoder, _draws: &Draws) -> u32 {
        0
    }
}

/// From the table's space to the key light's clip space for `frame`: the identity until the
/// shadow map is drawn.
pub(crate) fn light_view_projection(_frame: &Frame) -> Mat4 {
    Mat4::IDENTITY
}
