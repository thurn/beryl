//! Browses the stories the host provides and reserves a pane for the selected scene.

use std::convert::Infallible;
use std::sync::Arc;

use kit_base::data::flat::Gallery;
use kit_base::data::profiles::GalleryArrangement;
use kit_base::geometry::Window;
use kit_base::tokens::{color, space, type_style};
use kit_scene::{Context, Feature};
use kit_ui::element::{Align, Element, Justify, column, row, text};
use kit_ui::widgets::{button, list, screen::whole_window};

pub const PREVIEW: &str = "Story preview";

pub struct GalleryScreen;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inputs {
    pub window: Window,
    pub layout: Gallery,
    pub arrangement: GalleryArrangement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Story {
    pub name: String,
    pub feature: Option<String>,
}

#[derive(Debug, Default)]
pub struct State {
    stories: Arc<[Story]>,
    selected: Option<String>,
    details: Option<String>,
    timed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Stories(Arc<[Story]>),
    Open(String),
    Details { text: String, timed: bool },
    Restart,
}

pub enum Request {
    Open(String),
    Restart,
}

impl Feature for GalleryScreen {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = Infallible;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Infallible, Request>,
    ) -> bool {
        match message {
            Message::Stories(stories) => {
                state.stories = Arc::clone(stories);
                if state
                    .selected
                    .as_ref()
                    .is_some_and(|selected| stories.iter().any(|story| &story.name == selected))
                {
                    return true;
                }
                state.selected = stories
                    .iter()
                    .find(|story| story.name == "story/kit_ui/button-states")
                    .or_else(|| stories.first())
                    .map(|story| story.name.clone());
                state.details = None;
                if let Some(name) = &state.selected {
                    context.request(Request::Open(name.clone()));
                }
            }
            Message::Open(name) if state.stories.iter().any(|story| &story.name == name) => {
                state.selected = Some(name.clone());
                context.request(Request::Open(name.clone()));
            }
            Message::Details { text, timed } => {
                state.details = Some(text.clone());
                state.timed = *timed;
            }
            Message::Restart => context.request(Request::Restart),
            Message::Open(_) => return false,
        }
        true
    }

    fn view_key(_: &State, inputs: &Inputs) -> Inputs {
        *inputs
    }

    fn view(
        state: &State,
        Inputs {
            window,
            layout,
            arrangement,
        }: Inputs,
    ) -> Element {
        let catalog = list("Stories", story_rows(state, layout.row_height))
            .min_height(0.0)
            .min_width(0.0)
            .background(color::BACKDROP);
        let (content, catalog) = match arrangement {
            GalleryArrangement::SideBySide => (row(), catalog.width(layout.catalog_width)),
            GalleryArrangement::Stacked => (
                column(),
                catalog.height(
                    (window.size.height * layout.stacked_catalog_share).max(layout.row_height),
                ),
            ),
        };
        let details = state.details.as_deref().unwrap_or("No stories available");
        let viewer = column()
            .min_width(0.0)
            .min_height(0.0)
            .grow(1.0)
            .gap(space::S)
            .child(
                row()
                    .gap(space::S)
                    .align_items(Align::Center)
                    .justify_content(Justify::SpaceBetween)
                    .child(text(details).style(type_style::MENU).shrink(1.0))
                    .children(
                        state
                            .timed
                            .then(|| button("Replay").on_press(Message::Restart)),
                    ),
            )
            .child(
                Element::new(kit_frame::frame::Role::Region)
                    .name(PREVIEW)
                    .min_height(0.0)
                    .min_width(0.0)
                    .grow(1.0)
                    .background(color::BACKDROP),
            );
        let mut screen = column()
            .name("gallery")
            .background(color::SURFACE)
            .gap(space::S)
            .child(
                row()
                    .justify_content(Justify::SpaceBetween)
                    .align_items(Align::Center)
                    .child(text("Stories").style(type_style::TITLE))
                    .child(
                        button("Back").on_press(super::start::Message::Open(super::Screen::Start)),
                    ),
            )
            .child(
                content
                    .min_height(0.0)
                    .grow(1.0)
                    .gap(space::M)
                    .child(catalog)
                    .child(viewer),
            );
        let safe = window.safe_area;
        screen.style.padding = [
            safe.top + space::M,
            safe.right + space::M,
            safe.bottom + space::M,
            safe.left + space::M,
        ];
        whole_window(window.size, screen)
    }
}

fn story_rows(state: &State, row_height: f32) -> Vec<Element> {
    let mut group = None;
    let mut rows = Vec::new();
    for Story { name, feature } in &*state.stories {
        let path = name.strip_prefix("story/").unwrap_or(name);
        let (package, story) = path.split_once('/').unwrap_or(("", path));
        let category = (package, feature.as_deref());
        if group != Some(category) {
            group = Some(category);
            let heading = feature.as_ref().map_or_else(
                || package.to_owned(),
                |feature| format!("{package} / {feature}"),
            );
            rows.push(
                text(&heading)
                    .style(type_style::MENU)
                    .padding(space::S)
                    .shrink(0.0),
            );
        }
        let selected = state.selected.as_ref() == Some(name);
        let mut entry = Element::new(kit_frame::frame::Role::Button)
            .name(name)
            .child(text(story).style(type_style::MENU).shrink(1.0).passes(true))
            .padding(space::S)
            .min_height(row_height)
            .shrink(0.0)
            .selected(selected)
            .on_press(Message::Open(name.clone()));
        entry.focus = true;
        if selected {
            entry = entry.background(color::SURFACE);
        }
        rows.push(entry);
    }
    rows
}
