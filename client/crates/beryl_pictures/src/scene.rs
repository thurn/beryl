//! Adapts the inspector's scenes to the offscreen host's asset interface.

use std::ops::{Deref, DerefMut};

use beryl_host::{Assets, Headless, HeadlessPage};
use beryl_inspect::scene::Scene;
use beryl_render::Renderer;
use kit_base::data::lighting::Lighting;
use kit_frame::frame::Frame;

pub(crate) type SceneHost = Headless<PictureScene>;

pub(crate) struct PictureScene(pub Scene);

impl Deref for PictureScene {
    type Target = Scene;

    fn deref(&self) -> &Scene {
        &self.0
    }
}

impl DerefMut for PictureScene {
    fn deref_mut(&mut self) -> &mut Scene {
        &mut self.0
    }
}

impl HeadlessPage for PictureScene {
    fn with_host_atlas(self) -> Self {
        Self(self.0.with_host_atlas())
    }

    fn scale(&self) -> f32 {
        self.0.scale()
    }

    fn lighting(&self) -> Lighting {
        self.0.data().lighting
    }

    fn frame(&mut self) -> &Frame {
        self.0.frame()
    }

    fn prepare_assets(&mut self, assets: &mut Assets, renderer: &mut Renderer) {
        match &mut self.0 {
            Scene::Recording(page) => page.prepare_assets(assets, renderer),
            story @ Scene::Story(_) => {
                let data = story.story_data().cloned().expect("a story has its tuning");
                let (scale, wanted) = (story.scale(), story.wanted_glyphs());
                assets.settle_glyphs(renderer, &data, scale, wanted, |ready| {
                    story.give_glyphs(ready);
                });
            }
        }
    }
}
