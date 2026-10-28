//! The database layer: the queries that read the models and the commands that
//! write them. Nothing else lives here, and nothing above touches sqlx directly.

pub mod commands;
pub mod queries;
