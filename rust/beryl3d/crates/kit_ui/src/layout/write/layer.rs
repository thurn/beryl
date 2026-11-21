//! Writes a flat layer in paint order, including outgoing screens and floats.

use glam::Vec2;

use super::super::{Layer, float::Float, panel::Chain};
use super::{Drawing, FaceLayers, Look, children, over_floats, own_filter};
use kit_base::geometry::plane::{ScreenPlane, Zoom};
use kit_frame::frame::{Anchored, ElementId, FrameBuilder, StateBits};

impl Layer {
    /// Writes the flat layer into `builder`, on the plane `camera` puts in front of the table:
    /// the floats drawn under the screen (`FloatId::under_screen`), the screen, leaving out what
    /// waits for the table to settle (`Element::waits`) unless the layer is `open`, then every
    /// other float, then the screen's children that filter what is drawn under them
    /// (`over_floats`).
    pub fn write(&self, builder: &mut FrameBuilder, camera: &ScreenPlane, open: bool) {
        self.write_with(builder, camera, open, None);
    }

    /// `write`, drawing each face an element shows (`Shown::Face`) from the layers `faces`
    /// gives it. With no faces, such an element draws no face.
    pub fn write_with(
        &self,
        builder: &mut FrameBuilder,
        camera: &ScreenPlane,
        open: bool,
        faces: Option<&FaceLayers<'_>>,
    ) {
        // The recording path specializes away the preview callback.
        self.write_layer(builder, camera, open, faces, |_| StateBits(0));
    }

    /// Writes a toolkit preview with authored states applied to the same widgets the app uses.
    pub fn write_preview(
        &self,
        builder: &mut FrameBuilder,
        camera: &ScreenPlane,
        states: &dyn Fn(ElementId) -> StateBits,
    ) {
        self.write_layer(builder, camera, true, None, states);
    }

    fn write_layer(
        &self,
        builder: &mut FrameBuilder,
        camera: &ScreenPlane,
        open: bool,
        faces: Option<&FaceLayers<'_>>,
        preview: impl Fn(ElementId) -> StateBits + Copy,
    ) {
        let drawing = Drawing { camera, preview };
        // Two passes over the floats rather than a partition, which would allocate every frame.
        for float in (self.floats.iter()).filter(|float| float.id.under_screen()) {
            self.write_float(float, builder, drawing, open, faces);
        }
        let mut screen = None;
        let roots = self
            .departing
            .iter()
            .map(|node| (node, true))
            .chain(self.screen.iter().map(|node| (node, false)));
        for (node, departing) in roots {
            let point = match node.element.anchor {
                Some(_) => node.corner(),
                None => Some(Vec2::ZERO),
            };
            if let Some(point) = point {
                let look = Look {
                    point,
                    opacity: 1.0,
                    tint: None,
                    clip: None,
                    passes: false,
                    faces,
                    filters: Chain::default(),
                    open,
                    zoom: Zoom::IDENTITY,
                    departing,
                };
                let written = self.write_node(node, u32::MAX, true, look, builder, drawing);
                if !departing {
                    screen = written.map(|index| (node, index));
                }
            }
        }
        for float in (self.floats.iter()).filter(|float| !float.id.under_screen()) {
            self.write_float(float, builder, drawing, open, faces);
        }
        let Some((node, index)) = screen else {
            return;
        };
        for (child, departing) in children(node, false)
            .filter(|(child, _)| over_floats(child) && (open || !child.element.waits))
        {
            let Some(point) = child.corner() else {
                continue;
            };
            if !departing {
                builder.backdrop(&child.element.style.filter, self.window.scale);
            }
            let look = Look {
                point,
                opacity: node.opacity(),
                tint: None,
                clip: None,
                passes: node.element.style.passes,
                faces,
                filters: Chain::default().inner(own_filter(&node.element)),
                open,
                zoom: Zoom::IDENTITY,
                departing,
            };
            self.write_node(child, index, false, look, builder, drawing);
        }
    }

    fn write_float(
        &self,
        float: &Float,
        builder: &mut FrameBuilder,
        drawing: Drawing<'_, impl Fn(ElementId) -> StateBits + Copy>,
        open: bool,
        faces: Option<&FaceLayers<'_>>,
    ) {
        let rect = float.node.rect;
        let Some(corner) = float.corner() else {
            return;
        };
        let zoom = Zoom::around(
            corner + Vec2::new(rect.width, rect.height) / 2.0,
            float.look.scale,
        );
        let camera = drawing.camera.with_zoom(zoom);
        let look = Look {
            point: corner,
            opacity: float.look.opacity,
            tint: float.look.tint,
            clip: None,
            passes: false,
            faces,
            filters: Chain::default(),
            open,
            zoom,
            departing: false,
        };
        let element = self.write_node(
            &float.node,
            u32::MAX,
            true,
            look,
            builder,
            Drawing {
                camera: &camera,
                ..drawing
            },
        );
        if let (Some(element), Some(anchor)) = (element, &float.binding) {
            builder.anchored(Anchored {
                element,
                anchor: anchor.clone(),
                size: float.size(),
                scale: float.look.scale,
            });
        }
    }
}
