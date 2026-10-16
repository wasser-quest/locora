//! `locora tutorial`: the built-in tutorial palace "Locora lernen"
//! (built by `locora_runtime::tutorial`).

use anyhow::Result;
use locora_runtime::tutorial::{self, NAME};

use crate::app::App;

pub fn run(app: &mut App, reset: bool) -> Result<()> {
    if let Some(old) = tutorial::find(app)?
        && !reset
    {
        println!("Das Tutorial „{}“ ist schon da.", old.name);
        println!("  Üben: locora practice \"{NAME}\"   ·   neu anlegen: locora tutorial --reset");
        return Ok(());
    }
    let (tree, notes) = tutorial::install(app)?;
    crate::app::print(notes);
    println!(
        "✓ Tutorial „{NAME}“ angelegt: {} Räume, {} Orte.",
        tree.rooms.len(),
        tree.route().count()
    );
    println!();
    println!("  Das Tutorial ist selbst ein Gedächtnispalast. Geh die Route ab:");
    println!("  an jedem Ort steht eine Frage, du erinnerst dich an die Antwort.");
    println!("  In der App zeigt jeder Ort eine kleine Animation.");
    println!();
    println!("  Los geht's: locora practice \"{NAME}\"");
    Ok(())
}
