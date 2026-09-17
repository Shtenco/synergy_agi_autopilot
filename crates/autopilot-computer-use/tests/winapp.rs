use std::sync::Arc;

use async_trait::async_trait;
use autopilot_computer_use::{BrowserAction, BrowserRequest, ComputerUseAction, PlaywrightAdapter, WinAppAdapter, WindowsTarget};
use autopilot_runtime::{CommandResult, CommandSpec, RuntimeAdapter, RuntimeError};

struct NoopRuntime;

#[async_trait]
impl RuntimeAdapter for NoopRuntime {
    async fn probe(&self) -> bool { true }
    async fn execute(&self, _spec: CommandSpec) -> Result<CommandResult, RuntimeError> {
        Ok(CommandResult { exit_code: 0, stdout: "{}".into(), stderr: String::new(), duration_ms: 1 })
    }
    async fn cancel(&self) -> Result<(), RuntimeError> { Ok(()) }
}

#[test]
fn winapp_invoke_is_explicit_argv_and_targets_app() {
    let adapter = WinAppAdapter::new(Arc::new(NoopRuntime));
    let command = adapter.build_command(
        &WindowsTarget::App("notepad".into()),
        &ComputerUseAction::Invoke { selector: "btn-save-a1b2".into() },
    );
    assert_eq!(command.program, "winapp");
    assert_eq!(command.args, vec!["ui", "invoke", "btn-save-a1b2", "-a", "notepad", "--json"]);
}

#[test]
fn winapp_set_value_keeps_value_as_single_argument() {
    let adapter = WinAppAdapter::new(Arc::new(NoopRuntime));
    let command = adapter.build_command(
        &WindowsTarget::Hwnd(1234),
        &ComputerUseAction::SetValue { selector: "txt-name".into(), value: "hello; rm -rf /".into() },
    );
    assert_eq!(command.args[0..3], ["ui", "set-value", "txt-name"]);
    assert_eq!(command.args[3], "hello; rm -rf /");
    assert!(command.args.windows(2).any(|pair| pair == ["-w", "1234"]));
}

#[test]
fn only_uia_write_actions_are_marked_write() {
    assert!(ComputerUseAction::Invoke { selector: "x".into() }.is_write());
    assert!(ComputerUseAction::SetValue { selector: "x".into(), value: "y".into() }.is_write());
    assert!(!ComputerUseAction::Inspect.is_write());
    assert!(!ComputerUseAction::Screenshot { output: None }.is_write());
}

#[test]
fn browser_adapter_serializes_a_typed_request() {
    let adapter = PlaywrightAdapter::new(Arc::new(NoopRuntime), "tools/browser/bridge.mjs");
    let command = adapter.build_command(&BrowserRequest {
        action: BrowserAction::Fill { selector: "#email".into(), value: "user@example.com".into() },
        profile_dir: Some("C:/state/browser".into()),
    }).unwrap();
    assert_eq!(command.program, "node");
    assert_eq!(command.args[0], "tools/browser/bridge.mjs");
    assert!(command.args[1].contains("\"action\":\"fill\""));
    assert!(!command.args[1].contains("eval"));
}
