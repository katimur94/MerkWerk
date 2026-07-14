//! Local-AI distillation of captured sessions/snapshots into Markdown notes.
//!
//! Implements the "Destillierer" step of `docs/ROADMAP.md` Etappe 2: given
//! a time range, [`distill`] loads the sessions/snapshots captured in that
//! range from `storage::Store`, renders them into a compact text context
//! (budgeted via [`DistillerConfig`]), and asks a local model — behind the
//! `inference::Inference` trait per `ENTSCHEIDUNGEN.md` D9 — to turn that
//! context into a structured [`DistilledNote`]. Callers (the daemon, later
//! the "Jetzt destillieren" UI action) are responsible for actually writing
//! the resulting Markdown into the vault (`ENTSCHEIDUNGEN.md` D10) — this
//! crate only produces the note's content, it never touches the
//! filesystem.
//!
//! Platform-neutral and natively testable (`ENTSCHEIDUNGEN.md` D4/D6):
//! prompt/context construction is pure string logic tested against known
//! inputs, and [`distill`] itself is exercised end-to-end against
//! `storage::Store::open_in_memory` and `inference::MockInference` — no
//! real database file or running Ollama server required.

mod config;
mod context;
mod distill;
mod error;
mod prompt;

pub use config::{DistilledNote, DistillerConfig};
pub use context::{build_context, ms_to_hh_mm, truncate_chars, SessionContext};
pub use distill::distill;
pub use error::{Error, Result};
pub use prompt::build_prompt;
