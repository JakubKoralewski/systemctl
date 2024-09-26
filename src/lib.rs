//! Crate to manage and monitor services through `systemctl`   
//! Homepage: <https://github.com/gwbres/systemctl>
#![doc=include_str!("../README.md")]
use std::io::{Error, ErrorKind, Read};
use std::process::{Child, ExitStatus};
use std::str::FromStr;
use strum_macros::EnumString;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

const SYSTEMCTL_PATH: &str = "/usr/bin/systemctl";

use bon::Builder;

/// Struct with API calls to systemctl.
///
/// Use the `::default()` impl if you don't need special arguments.
///
/// Use the builder API when you want to specify a custom path to systemctl binary or extra args.
#[derive(Builder, Default, Clone, Debug)]
pub struct SystemCtl {
    /// Allows passing global arguments to systemctl like `--user`.
    additional_args: Vec<String>,
    /// The path to the systemctl binary, by default it's [SYSTEMCTL_PATH]
    path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: std::process::ExitStatus,
}

impl SystemCtl {
    /// Invokes `systemctl $args`
    fn spawn_child<'a, 's: 'a, S: IntoIterator<Item = &'a str>>(
        &'s self,
        args: S,
    ) -> std::io::Result<Child> {
        std::process::Command::new(self.get_path())
            .args(self.additional_args.iter().map(String::as_str).chain(args))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
    }

    fn get_path(&self) -> &str {
        self.path.as_deref().unwrap_or(SYSTEMCTL_PATH)
    }

    /// Invokes `systemctl $args` silently
    fn systemctl<'a, 's: 'a, S: IntoIterator<Item = &'a str>>(
        &'s self,
        args: S,
    ) -> std::io::Result<ExitStatus> {
        self.spawn_child(args)?.wait()
    }

    /// Invokes `systemctl $args` and captures stdout stream
    fn systemctl_capture<'a, 's: 'a, S: IntoIterator<Item = &'a str>>(
        &'s self,
        args: S,
    ) -> std::io::Result<RunResult> {
        let mut child = self.spawn_child(args)?;
        let exit_status = child.wait()?;
        match exit_status.code() {
            Some(0) => {}, // success
            Some(1) => {}, // success -> Ok(Unit not found)
            Some(3) => {}, // success -> Ok(unit is inactive and/or dead)
            Some(4) => {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    "Missing Priviledges or Unit not found",
                ))
            },
            // unknown errorcodes
            Some(code) => {
                return Err(Error::new(
                    // TODO: Maybe a better ErrorKind, none really seem to fit
                    ErrorKind::Other,
                    format!("Process exited with code: {code}"),
                ));
            },
            None => {
                return Err(Error::new(
                    ErrorKind::Interrupted,
                    "Process terminated by signal",
                ))
            },
        }

        let mut stdout = String::new();
        child.stdout.unwrap().read_to_string(&mut stdout).unwrap();

        let mut stderr = String::new();
        child.stderr.unwrap().read_to_string(&mut stderr).unwrap();

        Ok(RunResult {
            stdout,
            stderr,
            exit_status,
        })
    }

    /// Reloads all unit files
    pub fn daemon_reload(&self) -> std::io::Result<RunResult> {
        self.systemctl_capture(["daemon-reload"])
    }

    /// Forces given `unit` to (re)start
    pub fn restart(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["restart", unit])
    }

    /// Forces given `unit` to start
    pub fn start(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["start", unit])
    }

    /// Forces given `unit` to stop
    pub fn stop(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["stop", unit])
    }

    /// Forces given `unit` to stop
    pub fn clean(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["clean", unit])
    }

    /// Triggers reload for given `unit`
    pub fn reload(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["reload", unit])
    }

    /// Triggers reload or restarts given `unit`
    pub fn reload_or_restart(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["reload-or-restart", unit])
    }

    /// Enable given `unit` to start at boot
    pub fn enable(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["enable", unit])
    }

    /// Disable given `unit` to start at boot
    pub fn disable(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["disable", unit])
    }

    /// Returns raw status from `systemctl status $unit` call
    pub fn status(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["status", unit])
    }

    /// Invokes systemctl `cat` on given `unit`
    pub fn cat(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["cat", unit])
    }

    /// Returns `true` if given `unit` is actively running
    pub fn is_active(&self, unit: &str) -> std::io::Result<bool> {
        let status = self.systemctl_capture(["is-active", unit])?;
        Ok(status.stdout.trim_end().eq("active"))
    }

    /// Isolates given unit, only self and its dependencies are
    /// now actively running
    pub fn isolate(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["isolate", unit])
    }

    /// Freezes (halts) given unit.
    /// This operation might not be feasible.
    pub fn freeze(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["freeze", unit])
    }

    /// Unfreezes given unit (recover from halted state).
    /// This operation might not be feasible.
    pub fn unfreeze(&self, unit: &str) -> std::io::Result<RunResult> {
        self.systemctl_capture(["thaw", unit])
    }

    /// Returns `true` if given `unit` exists,
    /// ie., service could be or is actively deployed
    /// and manageable by systemd
    pub fn exists(&self, unit: &str) -> std::io::Result<bool> {
        let unit_list = self.list_units(None, None, Some(unit))?;
        Ok(!unit_list.is_empty())
    }

    /// Returns a `Vector` of `UnitList` structs extracted from systemctl listing.   
    ///  + type filter: optional `--type` filter
    ///  + state filter: optional `--state` filter
    ///  + glob filter: optional unit name filter
    pub fn list_units_full(
        &self,
        type_filter: Option<&str>,
        state_filter: Option<&str>,
        glob: Option<&str>,
    ) -> std::io::Result<Vec<UnitList>> {
        let mut args = vec!["list-unit-files"];
        if let Some(filter) = type_filter {
            args.push("--type");
            args.push(filter)
        }
        if let Some(filter) = state_filter {
            args.push("--state");
            args.push(filter)
        }
        if let Some(glob) = glob {
            args.push(glob)
        }
        let mut result: Vec<UnitList> = Vec::new();
        let content = self.systemctl_capture(args)?;
        let lines = content
            .stdout
            .lines()
            .filter(|line| line.contains('.') && !line.ends_with('.'));

        for l in lines {
            let parsed: Vec<&str> = l.split_ascii_whitespace().collect();
            let vendor_preset = match parsed[2] {
                "-" => None,
                "enabled" => Some(true),
                "disabled" => Some(false),
                _ => None,
            };
            result.push(UnitList {
                unit_file: parsed[0].to_string(),
                state: parsed[1].to_string(),
                vendor_preset,
            })
        }
        Ok(result)
    }

    /// Returns a `Vector` of unit names extracted from systemctl listing.   
    ///  + type filter: optional `--type` filter
    ///  + state filter: optional `--state` filter
    ///  + glob filter: optional unit name filter
    pub fn list_units(
        &self,
        type_filter: Option<&str>,
        state_filter: Option<&str>,
        glob: Option<&str>,
    ) -> std::io::Result<Vec<String>> {
        let list = self.list_units_full(type_filter, state_filter, glob);
        Ok(list?.iter().map(|n| n.unit_file.clone()).collect())
    }

    /// Returns list of services that are currently declared as disabled
    pub fn list_disabled_services(&self) -> std::io::Result<Vec<String>> {
        self.list_units(Some("service"), Some("disabled"), None)
    }

    /// Returns list of services that are currently declared as enabled
    pub fn list_enabled_services(&self) -> std::io::Result<Vec<String>> {
        self.list_units(Some("service"), Some("enabled"), None)
    }

    /// Builds a new `Unit` structure by retrieving
    /// structure attributes with a `systemctl status $unit` call
    pub fn create_unit(&self, name: &str) -> std::io::Result<Unit> {
        if let Ok(false) = self.exists(name) {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("Unit or service \"{}\" does not exist", name),
            ));
        }
        let mut u = Unit::default();
        let status = self.status(name)?;
        let mut lines = status.stdout.lines();
        let next = lines.next().unwrap();
        let (_, rem) = next.split_at(3);
        let mut items = rem.split_ascii_whitespace();
        let name_raw = items.next().unwrap().trim();
        if let Some(delim) = items.next() {
            if delim.trim().eq("-") {
                // --> description string is provided
                let items: Vec<_> = items.collect();
                u.description = Some(itertools::join(&items, " "));
            }
        }
        let (name, utype_raw) = name_raw
            .rsplit_once('.')
            .expect("Unit is missing a Type, this should not happen!");
        // `type` is deduced from .extension
        u.utype = match Type::from_str(utype_raw) {
            Ok(t) => t,
            Err(e) => panic!("For {:?} -> {e}", name_raw),
        };
        let mut is_doc = false;
        for line in lines {
            let line = line.trim_start();
            if let Some(line) = line.strip_prefix("Loaded: ") {
                // Match and get rid of "Loaded: "
                if let Some(line) = line.strip_prefix("loaded ") {
                    u.state = State::Loaded;
                    let line = line.strip_prefix('(').unwrap();
                    let line = line.strip_suffix(')').unwrap();
                    let items: Vec<&str> = line.split(';').collect();
                    u.script = items[0].trim().to_string();
                    u.auto_start = match AutoStartStatus::from_str(items[1].trim()) {
                        Ok(x) => x,
                        Err(_) => AutoStartStatus::Disabled,
                    };
                    if items.len() > 2 {
                        // preset is optionnal ?
                        u.preset = items[2].trim().ends_with("enabled");
                    }
                } else if line.starts_with("masked") {
                    u.state = State::Masked;
                }
            } else if let Some(line) = line.strip_prefix("Transient: ") {
                if line == "yes" {
                    u.transient = true
                }
            } else if line.starts_with("Active: ") {
                // skip that one
                // we already have .active() .inative() methods
                // to access this information
            } else if let Some(line) = line.strip_prefix("Docs: ") {
                is_doc = true;
                if let Ok(doc) = Doc::from_str(line) {
                    u.docs.get_or_insert_with(Vec::new).push(doc);
                }
            } else if let Some(line) = line.strip_prefix("What: ") {
                // mountpoint infos
                u.mounted = Some(line.to_string())
            } else if let Some(line) = line.strip_prefix("Where: ") {
                // mountpoint infos
                u.mountpoint = Some(line.to_string());
            } else if let Some(line) = line.strip_prefix("Main PID: ") {
                // example -> Main PID: 787 (gpm)
                if let Some((pid, proc)) = line.split_once(' ') {
                    u.pid = Some(pid.parse::<u64>().unwrap_or(0));
                    u.process = Some(proc.replace(&['(', ')'][..], ""));
                };
            } else if let Some(line) = line.strip_prefix("Cntrl PID: ") {
                // example -> Main PID: 787 (gpm)
                if let Some((pid, proc)) = line.split_once(' ') {
                    u.pid = Some(pid.parse::<u64>().unwrap_or(0));
                    u.process = Some(proc.replace(&['(', ')'][..], ""));
                };
            } else if line.starts_with("Process: ") {
                //TODO: implement
                //TODO: parse as a Process item
                //let items : Vec<_> = line.split_ascii_whitespace().collect();
                //let proc_pid = u64::from_str_radix(items[1].trim(), 10).unwrap();
                //let cli;
                //Process: 640 ExecStartPre=/usr/sbin/sshd -t (code=exited, status=0/SUCCESS)
            } else if line.starts_with("CGroup: ") {
                //TODO: implement
                //LINE: "CGroup: /system.slice/sshd.service"
                //LINE: "└─1050 /usr/sbin/sshd -D"
            } else if line.starts_with("Tasks: ") {
                //TODO: implement
            } else if let Some(line) = line.strip_prefix("Memory: ") {
                u.memory = Some(line.trim().to_string());
            } else if let Some(line) = line.strip_prefix("CPU: ") {
                u.cpu = Some(line.trim().to_string())
            } else {
                // handling multi line cases
                if is_doc {
                    let line = line.trim_start();
                    if let Ok(doc) = Doc::from_str(line) {
                        u.docs.get_or_insert_with(Vec::new).push(doc);
                    }
                }
            }
        }

        if let Ok(content) = self.cat(name) {
            let line_tuple = content
                .stdout
                .lines()
                .filter_map(|line| line.split_once('=').to_owned());
            for (k, v) in line_tuple {
                let val = v.to_string();
                match k {
                    "Wants" => u.wants.get_or_insert_with(Vec::new).push(val),
                    "WantedBy" => u.wanted_by.get_or_insert_with(Vec::new).push(val),
                    "Also" => u.also.get_or_insert_with(Vec::new).push(val),
                    "Before" => u.before.get_or_insert_with(Vec::new).push(val),
                    "After" => u.after.get_or_insert_with(Vec::new).push(val),
                    "ExecStart" => u.exec_start = Some(val),
                    "ExecReload" => u.exec_reload = Some(val),
                    "Restart" => u.restart_policy = Some(val),
                    "KillMode" => u.kill_mode = Some(val),
                    _ => {},
                }
            }
        }

        u.active = self.is_active(name)?;
        u.name = name.to_string();
        Ok(u)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
/// Implementation of list generated with
/// `systemctl list-unit-files`
pub struct UnitList {
    /// Unit name: `name.type`
    pub unit_file: String,
    /// Unit state
    pub state: String,
    /// Unit vendor preset
    pub vendor_preset: Option<bool>,
}

/// `AutoStartStatus` describes the Unit current state
#[derive(Copy, Clone, PartialEq, Eq, EnumString, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AutoStartStatus {
    #[strum(serialize = "static")]
    Static,
    #[strum(serialize = "enabled")]
    Enabled,
    #[strum(serialize = "enabled-runtime")]
    EnabledRuntime,
    #[strum(serialize = "disabled")]
    #[default]
    Disabled,
    #[strum(serialize = "generated")]
    Generated,
    #[strum(serialize = "indirect")]
    Indirect,
    #[strum(serialize = "transient")]
    Transient,
}

/// `Type` describes a Unit declaration Type in systemd
#[derive(Copy, Clone, PartialEq, Eq, EnumString, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Type {
    #[strum(serialize = "automount")]
    AutoMount,
    #[strum(serialize = "mount")]
    Mount,
    #[strum(serialize = "service")]
    #[default]
    Service,
    #[strum(serialize = "scope")]
    Scope,
    #[strum(serialize = "socket")]
    Socket,
    #[strum(serialize = "slice")]
    Slice,
    #[strum(serialize = "timer")]
    Timer,
    #[strum(serialize = "path")]
    Path,
    #[strum(serialize = "target")]
    Target,
}

/// `State` describes a Unit current state
#[derive(Copy, Clone, PartialEq, Eq, EnumString, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum State {
    #[strum(serialize = "masked")]
    #[default]
    Masked,
    #[strum(serialize = "loaded")]
    Loaded,
}

/*
/// Process
#[derive(Clone, Debug)]
pub struct Process {
    /// pid
    pid: u64,
    /// command line that was executed
    command: String,
    /// code
    code: String,
    /// status
    status: String,
}

impl Default for Process {
    fn default() -> Process {
        Process {
            pid: 0,
            command: Default::default(),
            code: Default::default(),
            status: Default::default(),
        }
    }
}*/

/// Doc describes types of documentation possibly
/// available for a systemd `unit`
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Doc {
    /// Man page is available
    Man(String),
    /// Webpage URL is indicated
    Url(String),
}

impl Doc {
    /// Unwrapps self as `Man` page
    pub fn as_man(&self) -> Option<&str> {
        match self {
            Doc::Man(s) => Some(s),
            _ => None,
        }
    }
    /// Unwrapps self as webpage `Url`
    pub fn as_url(&self) -> Option<&str> {
        match self {
            Doc::Url(s) => Some(s),
            _ => None,
        }
    }
}

impl std::str::FromStr for Doc {
    type Err = std::io::Error;
    /// Builds `Doc` from systemd status descriptor
    fn from_str(status: &str) -> Result<Self, Self::Err> {
        let items: Vec<&str> = status.split(':').collect();
        if items.len() != 2 {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "malformed doc descriptor",
            ));
        }
        match items[0] {
            "man" => {
                let content: Vec<&str> = items[1].split('(').collect();
                Ok(Doc::Man(content[0].to_string()))
            },
            "http" => Ok(Doc::Url("http:".to_owned() + items[1].trim())),
            "https" => Ok(Doc::Url("https:".to_owned() + items[1].trim())),
            _ => Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "unknown type of doc",
            )),
        }
    }
}

/// Structure to describe a systemd `unit`
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Unit {
    /// Unit name
    pub name: String,
    /// Unit type
    pub utype: Type,
    /// Optional unit description
    pub description: Option<String>,
    /// Current state
    pub state: State,
    /// Auto start feature
    pub auto_start: AutoStartStatus,
    /// `true` if Self is actively running
    pub active: bool,
    /// `true` if this unit is auto started by default,
    /// meaning, it should be manually disabled
    /// not to automatically start
    pub preset: bool,
    /// Configuration script loaded when starting this unit
    pub script: String,
    /// restart policy
    pub restart_policy: Option<String>,
    /// optionnal killmode info
    pub kill_mode: Option<String>,
    /// Optionnal process description (main tasklet "name")
    pub process: Option<String>,
    /// Optionnal process ID number (main tasklet pid)
    pub pid: Option<u64>,
    /// Running task(s) infos
    pub tasks: Option<u64>,
    /// Optionnal CPU load consumption infos
    pub cpu: Option<String>,
    /// Optionnal Memory consumption infos
    pub memory: Option<String>,
    /// mounted partition (`What`), if this is a `mount`/`automount` unit
    pub mounted: Option<String>,
    /// Mount point (`Where`), if this is a `mount`/`automount` unit
    pub mountpoint: Option<String>,
    /// Docs / `man` page(s) available for this unit
    pub docs: Option<Vec<Doc>>,
    /// wants attributes: list of other service / unit names
    pub wants: Option<Vec<String>>,
    /// wanted_by attributes: list of other service / unit names
    pub wanted_by: Option<Vec<String>>,
    /// also attributes
    pub also: Option<Vec<String>>,
    /// `before` attributes
    pub before: Option<Vec<String>>,
    /// `after` attributes
    pub after: Option<Vec<String>>,
    /// exec_start attribute: actual command line
    /// to be exected on `start` requests
    pub exec_start: Option<String>,
    /// exec_reload attribute, actual command line
    /// to be exected on `reload` requests
    pub exec_reload: Option<String>,
    /// If a command is run as transient service unit, it will be started and managed
    /// by the service manager like any other service, and thus shows up in the output
    /// of systemctl list-units like any other unit.
    pub transient: bool,
}

#[cfg(test)]
mod test {
    use super::*;

    fn ctl() -> SystemCtl {
        SystemCtl::default()
    }

    #[test]
    fn test_status_success() {
        let status = ctl().status("cron");
        println!("cron status: {:#?}", status);
        assert!(status.is_ok());
    }

    #[test]
    fn test_status_failure() {
        let status = ctl().status("not-existing");
        println!("not-existing status: {:#?}", status);
        assert!(status.is_err());
        let result = status.map_err(|e| e.kind());
        let expected = Err(ErrorKind::PermissionDenied);
        assert_eq!(expected, result);
    }

    #[test]
    fn test_is_active() {
        let units = ["sshd", "dropbear", "ntpd"];
        let ctl = ctl();
        for u in units {
            let active = ctl.is_active(u);
            println!("{} is-active: {:#?}", u, active);
            assert!(active.is_ok());
        }
    }
    #[test]
    fn test_service_exists() {
        let units = [
            "sshd",
            "dropbear",
            "ntpd",
            "example",
            "non-existing",
            "dummy",
        ];
        let ctl = ctl();
        for u in units {
            let ex = ctl.exists(u);
            println!("{} exists: {:#?}", u, ex);
            assert!(ex.is_ok());
        }
    }
    #[test]
    fn test_disabled_services() {
        let services = ctl().list_disabled_services().unwrap();
        println!("disabled services: {:#?}", services)
    }
    #[test]
    fn test_enabled_services() {
        let services = ctl().list_enabled_services().unwrap();
        println!("enabled services: {:#?}", services)
    }
    #[test]
    fn test_non_existing_unit() {
        let unit = ctl().create_unit("non-existing");
        assert!(unit.is_err());
        let result = unit.map_err(|e| e.kind());
        let expected = Err(ErrorKind::NotFound);
        assert_eq!(expected, result);
    }

    #[test]
    fn test_systemctl_exitcode_success() {
        let u = ctl().create_unit("cron.service");
        println!("{:#?}", u);
        assert!(u.is_ok());
    }

    #[test]
    fn test_systemctl_exitcode_not_found() {
        let u = ctl().create_unit("cran.service");
        println!("{:#?}", u);
        assert!(u.is_err());
        let result = u.map_err(|e| e.kind());
        let expected = Err(ErrorKind::NotFound);
        assert_eq!(expected, result);
    }

    #[test]
    fn test_service_unit_construction() {
        let ctl = ctl();
        let units = ctl.list_units(None, None, None).unwrap(); // all units
        for unit in units {
            let unit = unit.as_str();
            if unit.contains('@') {
                // not testing this one
                // would require @x service # identification / enumeration
                continue;
            }
            let c0 = unit.chars().next().unwrap();
            if c0.is_alphanumeric() {
                // valid unit name --> run test
                let u = ctl.create_unit(unit).unwrap();
                println!("####################################");
                println!("Unit: {:#?}", u);
                println!("active: {}", u.active);
                println!("preset: {}", u.preset);
                println!("auto_start (enabled): {:#?}", u.auto_start);
                println!("config script : {}", u.script);
                println!("pid: {:?}", u.pid);
                println!("Running task(s): {:?}", u.tasks);
                println!("Memory consumption: {:?}", u.memory);
                println!("####################################")
            }
        }
    }
    #[test]
    fn test_list_units_full() {
        let units = ctl().list_units_full(None, None, None).unwrap(); // all units
        for unit in units {
            println!("####################################");
            println!("Unit: {}", unit.unit_file);
            println!("State: {}", unit.state);
            println!("Vendor Preset: {:?}", unit.vendor_preset);
            println!("####################################");
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_for_unit() {
        let mut u = Unit::default();
        // make sure we test all enums
        u.docs
            .get_or_insert_with(Vec::new)
            .push(Doc::Man("some instruction".into()));
        u.auto_start = AutoStartStatus::Transient;
        u.state = State::Loaded;
        u.utype = Type::Socket;
        // serde
        let json_u = serde_json::to_string(&u).unwrap();
        let reverse = serde_json::from_str(&json_u).unwrap();
        assert_eq!(u, reverse);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_for_unit_list() {
        let u = UnitList::default();
        // serde
        let json_u = serde_json::to_string(&u).unwrap();
        let reverse = serde_json::from_str(&json_u).unwrap();
        assert_eq!(u, reverse);
    }
}
