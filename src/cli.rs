//! Command line surface.
//!
//! `--version` and `--help` are handled by the argument parser before anything
//! else runs, so they never initialize the GUI or touch the network.
//!
//! The inspection subcommands exist so every read-only behavior can be checked
//! from a terminal, which is also how the bootstrap verification was performed.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::identity;

/// The parsed command line.
#[derive(Debug, Parser)]
#[command(
    name = identity::PACKAGE,
    version = identity::VERSION,
    about = identity::DESCRIPTION,
    long_about = "A native Android virtual device manager.\n\n\
                  Android emulators, without the IDE.\n\n\
                  Avdium has no telemetry. It never uses sudo, and it does not \
                  require a system-wide Java installation."
)]
pub struct Cli {
    /// Subcommands that report and exit instead of opening the window.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Start in demo mode: synthetic devices, and no access to a real SDK.
    #[arg(long)]
    pub demo: bool,

    /// Use this SDK root instead of the discovered one.
    #[arg(long, value_name = "PATH")]
    pub sdk: Option<PathBuf>,

    /// Use this AVD home instead of the discovered one.
    #[arg(long, value_name = "PATH")]
    pub avd_home: Option<PathBuf>,

    /// Log level for Avdium's own logs.
    #[arg(long, value_name = "LEVEL")]
    pub log: Option<String>,

    /// Write repeatable screenshots to this directory and exit.
    ///
    /// Requires a display server: Wayland, X11, or a virtual display such as
    /// Xvfb. See docs/VALIDATION.md for which renderer is implemented.
    #[cfg(feature = "screenshot")]
    #[arg(long, value_name = "DIR")]
    pub screenshot: Option<PathBuf>,
}

/// Reporting subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the resolved SDK and AVD paths.
    Paths {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print the detected tools and their versions.
    Tools {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Run the doctor and print the result.
    Doctor {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Also run the checks that require executing a tool.
        #[arg(long)]
        probe: bool,
    },
    /// List discovered devices.
    Devices {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print the capability matrix for the detected providers.
    Capabilities {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_arguments_means_open_the_window() {
        let cli = Cli::parse_from(["avdium"]);
        assert!(cli.command.is_none());
        assert!(!cli.demo);
        assert!(cli.sdk.is_none());
    }

    #[test]
    fn paths_accepts_an_sdk_override() {
        let cli = Cli::parse_from(["avdium", "--sdk", "/opt/sdk", "paths", "--json"]);
        assert_eq!(cli.sdk, Some(PathBuf::from("/opt/sdk")));
        assert!(matches!(cli.command, Some(Command::Paths { json: true })));
    }

    #[test]
    fn demo_mode_is_opt_in() {
        let cli = Cli::parse_from(["avdium", "--demo"]);
        assert!(cli.demo);
    }

    #[test]
    fn unknown_arguments_are_rejected_rather_than_ignored() {
        assert!(Cli::try_parse_from(["avdium", "--not-a-flag"]).is_err());
        assert!(Cli::try_parse_from(["avdium", "not-a-command"]).is_err());
    }

    #[test]
    fn every_reporting_subcommand_has_a_json_mode() {
        for name in ["paths", "tools", "doctor", "devices", "capabilities"] {
            let cli = Cli::try_parse_from(["avdium", name, "--json"]).expect(name);
            assert!(cli.command.is_some());
        }
    }

    #[test]
    fn version_output_contains_the_product_version() {
        let error = Cli::try_parse_from(["avdium", "--version"]).expect_err("version exits");
        let text = error.to_string();
        assert!(text.contains(identity::VERSION), "{text}");
    }
}
