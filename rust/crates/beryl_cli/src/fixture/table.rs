//! Records a solo table through the same session and action references as the app.

use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::channel;

use beryl_session::ipc::{self, FixtureMessage};
use beryl_session::{GameUpdate, SessionIds};
use beryl_table::{Action, deck};
use beryl_table_session::TableSessions;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    deck: String,
    // Use the handle number from the preceding view. The session issues a new
    // view token every time, even when its deck shuffle and actions are identical.
    actions: Vec<Action<usize>>,
}

pub(super) fn record(
    definition: &Definition,
    source: &Path,
) -> Result<Vec<FixtureMessage>, String> {
    let (told, panic) = channel();
    std::panic::catch_unwind(|| {
        let _guard = crate::panics::Guard::new(move |message| drop(told.send(message)));
        play(definition, source)
    })
    .unwrap_or_else(|_| {
        Err(panic
            .try_recv()
            .unwrap_or_else(|_| "the table panicked".into()))
    })
}

fn deck_path(name: &str, source: &Path) -> Result<PathBuf, String> {
    let file = Path::new(name);
    let mut components = file.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err("a table deck must name a file in docs/sample-decks".into());
    }
    let source = source
        .canonicalize()
        .map_err(|e| format!("{}: {e}", source.display()))?;
    let current = std::env::current_dir().map_err(|e| e.to_string())?;
    // Temporary definitions can also refer to decks in the current checkout.
    source
        .ancestors()
        .skip(1)
        .chain(current.ancestors())
        .map(|dir| dir.join("docs/sample-decks"))
        .find(|dir| dir.is_dir())
        .map(|dir| dir.join(file))
        .ok_or_else(|| "could not locate docs/sample-decks".into())
}

fn play(definition: &Definition, source: &Path) -> Result<Vec<FixtureMessage>, String> {
    let path = deck_path(&definition.deck, source)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let deck = deck::parse_text(&text);
    if !deck.skipped.is_empty() {
        return Err(format!(
            "unrecognized deck lines in {}: {:?}",
            definition.deck, deck.skipped
        ));
    }

    let (emit, updates) = channel();
    let sessions = TableSessions::new(SessionIds::default(), move |update| {
        emit.send(update).expect("the recorder is listening");
    });
    let session = sessions
        .solo_seeded(deck, 0)
        .map_err(|e| format!("table_solo was refused: {e:?}"))?;
    let next = || {
        updates
            .try_recv()
            .map_err(|_| "the table sent no view".to_owned())
    };
    let mut update = next()?;
    let mut messages = vec![
        FixtureMessage::response(ipc::TABLE_SOLO, &session),
        FixtureMessage::game_update(&update),
    ];
    for (index, action) in definition.actions.iter().enumerate() {
        update = act(&sessions, &update, action.clone())
            .and_then(|()| next())
            .map_err(|e| format!("table action {} ({action:?}): {e}", index + 1))?;
        messages.push(FixtureMessage::game_update(&update));
    }
    Ok(messages)
}

fn act(sessions: &TableSessions, update: &GameUpdate, action: Action<usize>) -> Result<(), String> {
    let table = update
        .view
        .table
        .as_ref()
        .ok_or("expected a manual table view")?;
    let action = action.try_map_objects(&mut |handle, zone| {
        table
            .object_actions
            .get(handle)
            .filter(|entry| entry.zone == zone)
            .map(|entry| entry.reference.clone())
            .ok_or_else(|| format!("the view has no action handle {handle} in {zone:?}"))
    })?;
    sessions
        .act(action)
        .map_err(|e| format!("table_act was refused: {e:?}"))
}
