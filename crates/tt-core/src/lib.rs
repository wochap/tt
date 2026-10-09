//! tt domain core: document schemas over Automerge, task/entry operations,
//! event derivation, time parsing, reports, rounding, export, and search.

pub mod am;
pub mod error;
pub mod events;
pub mod export;
pub mod model;
pub mod ops;
pub mod report;
pub mod round;
pub mod schema;
pub mod search;
pub mod text;
pub mod time;

pub use error::{CoreError, CoreResult};
pub use model::{
    Entry, EntryView, Index, Project, RenumberedFrom, Tag, Task, TaskState, TaskView, View,
    Workspace,
};
