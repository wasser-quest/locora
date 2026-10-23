//! CLI context: an open profile (see `locora-runtime`) that prints plugin
//! problems to stderr instead of returning them.

use std::ops::{Deref, DerefMut};
use std::path::Path;

use anyhow::Result;
use locora_runtime::{AnswerRequest, Answered, Profile};
pub use locora_runtime::{data_dir, embedded, now, plugin_dir};

pub struct App(Profile);

impl Deref for App {
    type Target = Profile;
    fn deref(&self) -> &Profile {
        &self.0
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Profile {
        &mut self.0
    }
}

pub fn print(notes: impl IntoIterator<Item = String>) {
    for note in notes {
        eprintln!("⚠ {note}");
    }
}

impl App {
    pub fn open(data: Option<&Path>) -> Result<Self> {
        let mut profile = Profile::open(data)?;
        print(std::mem::take(&mut profile.notes));
        Ok(Self(profile))
    }

    /// Publish an event and show what plugins reported (only problems).
    pub fn publish<E: locora_api::Event>(&mut self, event: &E) -> Result<()> {
        print(self.0.publish(event)?);
        Ok(())
    }

    /// Print plugin warnings and errors; normal plugin chatter stays quiet.
    pub fn report(&mut self) {
        print(self.0.report());
    }

    /// Grade an answer (see `Profile::answer`) and print plugin problems.
    pub fn answer(&mut self, request: AnswerRequest<'_>) -> Result<Answered> {
        let mut answered = self.0.answer(request)?;
        print(std::mem::take(&mut answered.notes));
        Ok(answered)
    }
}

#[cfg(test)]
impl App {
    /// An in-memory profile without plugins, for unit tests.
    pub fn for_tests() -> Self {
        Self(Profile::from_parts(
            std::sync::Arc::new(locora_store::Store::in_memory().unwrap()),
            std::env::temp_dir(),
            locora_host::PluginHost::new(None).unwrap(),
        ))
    }
}
