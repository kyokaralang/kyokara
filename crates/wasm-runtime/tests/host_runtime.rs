#![allow(clippy::unwrap_used)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use kyokara_runtime::replay::{ReplayLogConfig, ReplayLogLine, ReplayReader, WASM_RUNTIME};
use kyokara_runtime::service::{HostBackend, LiveRuntime, ReplayMode, build_replay_header};
use kyokara_wasm_runtime::{HostStatus, WasmProgram};

#[derive(Default)]
struct TestHostState {
    printed: Vec<(String, bool)>,
    line: String,
    stdin: String,
    files: HashMap<String, String>,
}

struct TestHostBackend {
    state: Rc<RefCell<TestHostState>>,
}

impl HostBackend for TestHostBackend {
    fn print(&mut self, text: &str, newline: bool) -> Result<(), String> {
        self.state
            .borrow_mut()
            .printed
            .push((text.to_string(), newline));
        Ok(())
    }

    fn read_line(&mut self) -> Result<String, String> {
        Ok(self.state.borrow().line.clone())
    }

    fn read_stdin(&mut self) -> Result<String, String> {
        Ok(self.state.borrow().stdin.clone())
    }

    fn read_file(&mut self, path: &str) -> Result<String, String> {
        self.state
            .borrow()
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("missing `{path}`"))
    }
}

fn wasm_header(entry_file: &Path) -> kyokara_runtime::replay::ReplayHeader {
    build_replay_header(entry_file, false, [entry_file.to_path_buf()], WASM_RUNTIME).unwrap()
}

fn println_module() -> &'static str {
    r#"(module
  (import "kyokara_host" "capability_authorize" (func $auth (param i32 i32 i32 i32 i32) (result i32)))
  (import "kyokara_host" "io_println" (func $println (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "io")
  (data (i32.const 16) "io.println")
  (data (i32.const 64) "hello")
  (func (export "main") (result i32)
    (local $status i32)
    i32.const 0
    i32.const 2
    i32.const 0
    i32.const 16
    i32.const 10
    call $auth
    local.tee $status
    if (result i32)
      local.get $status
    else
      i32.const 64
      i32.const 5
      i32.const 16
      i32.const 10
      call $println
    end))"#
}

fn read_line_module(import_name: &str, required_by_name: &str) -> String {
    format!(
        r#"(module
  (import "kyokara_host" "capability_authorize" (func $auth (param i32 i32 i32 i32 i32) (result i32)))
  (import "kyokara_host" "{import_name}" (func $reader (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "io")
  (data (i32.const 16) "{required_by_name}")
  (func (export "main") (result i32)
    (local $status i32)
    i32.const 0
    i32.const 2
    i32.const 0
    i32.const 16
    i32.const {name_len}
    call $auth
    local.tee $status
    if (result i32)
      local.get $status
    else
      i32.const 128
      i32.const 64
      i32.const 256
      i32.const 16
      i32.const {name_len}
      call $reader
      local.tee $status
      if (result i32)
        local.get $status
      else
        i32.const 256
        i32.load
      end
    end))"#,
        name_len = required_by_name.len()
    )
}

fn read_file_module(path: &str) -> String {
    format!(
        r#"(module
  (import "kyokara_host" "capability_authorize" (func $auth (param i32 i32 i32 i32 i32) (result i32)))
  (import "kyokara_host" "fs_read_file" (func $read_file (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "fs")
  (data (i32.const 16) "fs.read_file")
  (data (i32.const 64) "{path}")
  (func (export "main") (result i32)
    (local $status i32)
    i32.const 0
    i32.const 2
    i32.const 0
    i32.const 16
    i32.const 12
    call $auth
    local.tee $status
    if (result i32)
      local.get $status
    else
      i32.const 64
      i32.const {path_len}
      i32.const 128
      i32.const 64
      i32.const 256
      i32.const 16
      i32.const 12
      call $read_file
      local.tee $status
      if (result i32)
        local.get $status
      else
        i32.const 256
        i32.load
      end
    end))"#,
        path_len = path.len()
    )
}

#[test]
fn wasm_host_print_logs_and_replay_modes_match_runtime_contract() {
    let dir = tempfile::tempdir().unwrap();
    let entry_path = dir.path().join("main.ky");
    let log_path = dir.path().join("run.jsonl");
    fs::write(&entry_path, "fn main() -> Unit {}\n").unwrap();

    let state = Rc::new(RefCell::new(TestHostState::default()));
    let live = LiveRuntime::new(
        Box::new(TestHostBackend {
            state: state.clone(),
        }),
        Box::new(|_| true),
        Some(ReplayLogConfig {
            path: log_path.clone(),
            header: wasm_header(&entry_path),
        }),
    )
    .unwrap();

    let mut program =
        WasmProgram::instantiate_with_runtime(println_module().as_bytes(), Box::new(live)).unwrap();
    assert_eq!(program.call_main_i32().unwrap(), HostStatus::Ok as i32);
    assert_eq!(state.borrow().printed, vec![("hello".to_string(), true)]);

    let mut reader = ReplayReader::from_path(&log_path).unwrap();
    assert_eq!(reader.header().runtime, WASM_RUNTIME);
    assert!(matches!(
        reader.next_event().unwrap(),
        Some(ReplayLogLine::CapabilityCheck(_))
    ));
    assert!(matches!(
        reader.next_event().unwrap(),
        Some(ReplayLogLine::EffectCall(_))
    ));

    let (mut replay_program, header) = WasmProgram::instantiate_with_replay_log(
        println_module().as_bytes(),
        &log_path,
        ReplayMode::Replay,
    )
    .unwrap();
    assert_eq!(header.runtime, WASM_RUNTIME);
    assert_eq!(
        replay_program.call_main_i32().unwrap(),
        HostStatus::Ok as i32
    );
    assert!(replay_program.last_host_error().is_none());

    let mutated = fs::read_to_string(&log_path)
        .unwrap()
        .replace("\"hello\"", "\"goodbye\"");
    fs::write(&log_path, mutated).unwrap();

    let (mut verify_program, _) = WasmProgram::instantiate_with_replay_log(
        println_module().as_bytes(),
        &log_path,
        ReplayMode::Verify,
    )
    .unwrap();
    assert_eq!(
        verify_program.call_main_i32().unwrap(),
        HostStatus::RuntimeError as i32
    );
    assert!(
        verify_program
            .last_host_error()
            .is_some_and(|msg| msg.contains("mismatch")),
        "error: {:?}",
        verify_program.last_host_error()
    );
}

#[test]
fn wasm_host_read_effects_fill_guest_memory_and_replay_logged_reads() {
    let dir = tempfile::tempdir().unwrap();
    let entry_path = dir.path().join("main.ky");
    let log_path = dir.path().join("read.jsonl");
    fs::write(&entry_path, "fn main() -> Unit {}\n").unwrap();

    let state = Rc::new(RefCell::new(TestHostState {
        line: "alpha".to_string(),
        stdin: "stdin text".to_string(),
        ..Default::default()
    }));
    let live = LiveRuntime::new(
        Box::new(TestHostBackend {
            state: state.clone(),
        }),
        Box::new(|_| true),
        Some(ReplayLogConfig {
            path: log_path.clone(),
            header: wasm_header(&entry_path),
        }),
    )
    .unwrap();

    let read_line_wat = read_line_module("io_read_line", "io.read_line");
    let mut line_program =
        WasmProgram::instantiate_with_runtime(read_line_wat.as_bytes(), Box::new(live)).unwrap();
    let len = line_program.call_main_i32().unwrap();
    assert_eq!(len, 5);
    assert_eq!(
        String::from_utf8(line_program.read_memory(128, len as u32).unwrap()).unwrap(),
        "alpha"
    );

    let (mut replay_program, _) = WasmProgram::instantiate_with_replay_log(
        read_line_wat.as_bytes(),
        &log_path,
        ReplayMode::Replay,
    )
    .unwrap();
    let replay_len = replay_program.call_main_i32().unwrap();
    assert_eq!(replay_len, 5);
    assert_eq!(
        String::from_utf8(replay_program.read_memory(128, replay_len as u32).unwrap()).unwrap(),
        "alpha"
    );

    let stdin_log = dir.path().join("stdin.jsonl");
    let stdin_live = LiveRuntime::new(
        Box::new(TestHostBackend { state }),
        Box::new(|_| true),
        Some(ReplayLogConfig {
            path: stdin_log.clone(),
            header: wasm_header(&entry_path),
        }),
    )
    .unwrap();
    let read_stdin_wat = read_line_module("io_read_stdin", "io.read_stdin");
    let mut stdin_program =
        WasmProgram::instantiate_with_runtime(read_stdin_wat.as_bytes(), Box::new(stdin_live))
            .unwrap();
    let stdin_len = stdin_program.call_main_i32().unwrap();
    assert_eq!(stdin_len, 10);
    assert_eq!(
        String::from_utf8(stdin_program.read_memory(128, stdin_len as u32).unwrap()).unwrap(),
        "stdin text"
    );
}

#[test]
fn wasm_host_read_file_uses_runtime_service_and_capability_denials_surface_as_status() {
    let dir = tempfile::tempdir().unwrap();
    let entry_path = dir.path().join("main.ky");
    fs::write(&entry_path, "fn main() -> Unit {}\n").unwrap();

    let file_path = dir.path().join("input.txt");
    let file_text = "from fs";
    let mut files = HashMap::new();
    files.insert(file_path.display().to_string(), file_text.to_string());
    let state = Rc::new(RefCell::new(TestHostState {
        files,
        ..Default::default()
    }));
    let read_file_wat = read_file_module(&file_path.display().to_string());

    let live = LiveRuntime::new(
        Box::new(TestHostBackend {
            state: state.clone(),
        }),
        Box::new(|_| true),
        None,
    )
    .unwrap();
    let mut ok_program =
        WasmProgram::instantiate_with_runtime(read_file_wat.as_bytes(), Box::new(live)).unwrap();
    let len = ok_program.call_main_i32().unwrap();
    assert_eq!(len, file_text.len() as i32);
    assert_eq!(
        String::from_utf8(ok_program.read_memory(128, len as u32).unwrap()).unwrap(),
        file_text
    );

    let denied = LiveRuntime::new(
        Box::new(TestHostBackend { state }),
        Box::new(|_| false),
        None,
    )
    .unwrap();
    let mut denied_program =
        WasmProgram::instantiate_with_runtime(read_file_wat.as_bytes(), Box::new(denied)).unwrap();
    assert_eq!(
        denied_program.call_main_i32().unwrap(),
        HostStatus::CapabilityDenied as i32
    );
}
