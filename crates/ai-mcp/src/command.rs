use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    TranscriptFixture,
    PushToTalkAsr,
    #[default]
    Typed,
    ModelPlan,
    SkillReplay,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct RouteCommandParams {
    pub text: String,
    #[serde(default)]
    pub source: CommandSource,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct SkillRegistryParams {
    #[serde(default)]
    pub include_draft: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct SkillAcquisitionParams {
    pub app: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub observed_window_title: Option<String>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CommandEnvelope {
    pub raw_text: String,
    pub normalized_text: String,
    pub confidence: Option<f32>,
    pub source: CommandSource,
    pub parser: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RoutedCommand {
    pub envelope: CommandEnvelope,
    pub intent: CommandIntent,
    pub slots: BTreeMap<String, String>,
    pub safety: SafetyDecision,
    pub action: PlannedAction,
    pub result: CommandResult,
    pub audit: AuditEntry,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Future router schema variants are intentionally visible before every executor exists.
pub enum CommandIntent {
    OpenApp,
    OpenUrl,
    FocusWindow,
    MoveWindow,
    ResizeWindow,
    SwitchWorkspace,
    RunAppSkill,
    AskClarification,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // M7 exposes the full action envelope while some operations remain planned-only.
pub enum PlannedAction {
    LaunchApp {
        command: String,
        args: Vec<String>,
    },
    OpenUrl {
        command: String,
        args: Vec<String>,
    },
    FocusWindow {
        window: u32,
    },
    TileWindow {
        window: Option<u32>,
        slot: String,
    },
    ResizeWindow {
        window: Option<u32>,
        width: u32,
        height: u32,
    },
    SwitchWorkspace {
        workspace: u32,
    },
    RunSkill {
        skill_id: String,
        operation: String,
        command: Option<String>,
        args: Vec<String>,
    },
    Clarify {
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SafetyDecision {
    pub risk: RiskLevel,
    pub confirmation_required: bool,
    pub allowed_without_confirmation: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Higher-risk classes are part of the public safety contract before execution support exists.
pub enum RiskLevel {
    LocalNavigation,
    LocalMutation,
    ExternalSendOrPost,
    CredentialOrPayment,
    DestructiveSystem,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CommandResult {
    pub executed: bool,
    pub ok: bool,
    pub status: String,
    pub details: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct AuditEntry {
    pub source: CommandSource,
    pub normalized_intent: String,
    pub confirmation_state: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct AppSkillRegistry {
    pub schema_version: u16,
    pub skills: Vec<AppSkill>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct AppSkill {
    pub id: String,
    pub display_name: String,
    pub status: String,
    pub version: String,
    pub provenance: String,
    pub launch_aliases: Vec<String>,
    pub operations: Vec<SkillOperation>,
    pub control_surfaces: Vec<String>,
    pub observation_adapters: Vec<String>,
    pub prerequisites: Vec<String>,
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SkillOperation {
    pub name: String,
    pub description: String,
    pub risk: RiskLevel,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SkillAcquisitionPlan {
    pub app: String,
    pub status: String,
    pub steps: Vec<String>,
    pub required_evidence: Vec<String>,
    pub trust_promotion_gate: Vec<String>,
    pub rollback: Vec<String>,
}

pub fn route(params: &RouteCommandParams) -> RoutedCommand {
    let normalized = normalize_command(&params.text);
    let (intent, slots, action) = parse_command(&normalized);
    let safety = classify_action(&action);
    let confirmation_state = if safety.confirmation_required {
        if params.confirmed {
            "confirmed"
        } else {
            "pending_confirmation"
        }
    } else {
        "not_required"
    };
    let result = if matches!(action, PlannedAction::Clarify { .. }) {
        CommandResult {
            executed: false,
            ok: false,
            status: "clarification_required".to_string(),
            details: BTreeMap::new(),
        }
    } else if params.dry_run || (safety.confirmation_required && !params.confirmed) {
        CommandResult {
            executed: false,
            ok: !matches!(action, PlannedAction::Clarify { .. }),
            status: if params.dry_run {
                "dry_run".to_string()
            } else if safety.confirmation_required {
                "confirmation_required".to_string()
            } else {
                "planned".to_string()
            },
            details: BTreeMap::new(),
        }
    } else {
        CommandResult {
            executed: false,
            ok: true,
            status: "ready_to_execute".to_string(),
            details: BTreeMap::new(),
        }
    };

    RoutedCommand {
        envelope: CommandEnvelope {
            raw_text: params.text.clone(),
            normalized_text: normalized,
            confidence: params.confidence,
            source: params.source.clone(),
            parser: "deterministic-v1".to_string(),
        },
        audit: AuditEntry {
            source: params.source.clone(),
            normalized_intent: format!("{intent:?}").to_ascii_lowercase(),
            confirmation_state: confirmation_state.to_string(),
            dry_run: params.dry_run,
        },
        intent,
        slots,
        safety,
        action,
        result,
    }
}

pub fn skills(include_draft: bool) -> AppSkillRegistry {
    let mut skills = vec![
        AppSkill {
            id: "browser.managed".to_string(),
            display_name: "Managed browser".to_string(),
            status: "trusted".to_string(),
            version: "2026-07-03.1".to_string(),
            provenance: "wmaker-ng seed skill".to_string(),
            launch_aliases: vec![
                "browser".to_string(),
                "brave".to_string(),
                "chrome".to_string(),
                "web".to_string(),
            ],
            operations: vec![
                SkillOperation {
                    name: "open_url".to_string(),
                    description: "Open a URL in the managed browser/profile lane.".to_string(),
                    risk: RiskLevel::LocalNavigation,
                },
                SkillOperation {
                    name: "summarize_controls".to_string(),
                    description: "Use browser native messaging for DOM/ARIA controls.".to_string(),
                    risk: RiskLevel::LocalNavigation,
                },
            ],
            control_surfaces: vec![
                "desktop-entry-or-executable".to_string(),
                "browser-native-messaging".to_string(),
                "x11-input-fallback".to_string(),
            ],
            observation_adapters: vec![
                "browser_native_messaging".to_string(),
                "observe".to_string(),
            ],
            prerequisites: vec!["managed browser launcher configured".to_string()],
            examples: vec![
                "open browser to linkedin.com".to_string(),
                "open chrome".to_string(),
            ],
        },
        AppSkill {
            id: "blender.procedural".to_string(),
            display_name: "Blender procedural control".to_string(),
            status: "trusted_fixture".to_string(),
            version: "2026-07-03.1".to_string(),
            provenance: "wmaker-ng seed skill for M7 Blender proof".to_string(),
            launch_aliases: vec!["blender".to_string()],
            operations: vec![
                SkillOperation {
                    name: "render_cylinder".to_string(),
                    description:
                        "Create a cylinder scene with Blender Python and render an artifact."
                            .to_string(),
                    risk: RiskLevel::LocalMutation,
                },
                SkillOperation {
                    name: "inspect_gui".to_string(),
                    description: "Launch Blender GUI and inspect the window/control surface."
                        .to_string(),
                    risk: RiskLevel::LocalNavigation,
                },
            ],
            control_surfaces: vec![
                "blender-python-api".to_string(),
                "command-line-background-render".to_string(),
                "at-spi".to_string(),
                "x11-input-fallback".to_string(),
                "vision-fallback".to_string(),
            ],
            observation_adapters: vec![
                "accessibility_tree".to_string(),
                "desktop_scene".to_string(),
                "observe".to_string(),
            ],
            prerequisites: vec!["blender executable on PATH".to_string()],
            examples: vec!["make a cylinder in blender and render it".to_string()],
        },
    ];

    if include_draft {
        skills.push(AppSkill {
            id: "unknown.discovery".to_string(),
            display_name: "Unknown app discovery".to_string(),
            status: "draft_template".to_string(),
            version: "2026-07-03.1".to_string(),
            provenance: "generated skill acquisition loop template".to_string(),
            launch_aliases: vec![],
            operations: vec![SkillOperation {
                name: "draft_skill".to_string(),
                description: "Collect app/version/control-surface evidence and create a reviewable skill draft.".to_string(),
                risk: RiskLevel::LocalNavigation,
            }],
            control_surfaces: vec!["cli".to_string(), "dbus".to_string(), "at-spi".to_string(), "x11".to_string()],
            observation_adapters: vec!["desktop_scene".to_string(), "accessibility_tree".to_string(), "screenshot".to_string()],
            prerequisites: vec!["operator approval before trusting learned skills".to_string()],
            examples: vec![],
        });
    }

    AppSkillRegistry {
        schema_version: 1,
        skills,
    }
}

pub fn acquisition_plan(params: &SkillAcquisitionParams) -> SkillAcquisitionPlan {
    SkillAcquisitionPlan {
        app: params.app.clone(),
        status: "draft_only".to_string(),
        steps: vec![
            "identify executable, desktop entry, package version, and process/window identity"
                .to_string(),
            "enumerate CLI flags, DBus services, scripting APIs, and app-specific automation hooks"
                .to_string(),
            "sample desktop_scene and accessibility_tree before using pixels".to_string(),
            "run bounded probes with dry-run/explain output where possible".to_string(),
            "write a draft skill manifest with operations, slots, risks, and fixtures".to_string(),
        ],
        required_evidence: vec![
            "command transcript or typed fixture".to_string(),
            "observed window/process identity".to_string(),
            "control surfaces tried and results".to_string(),
            "semantic tree or screenshots when semantics are unavailable".to_string(),
            "artifact metadata for produced files".to_string(),
        ],
        trust_promotion_gate: vec![
            "human review of draft manifest".to_string(),
            "repeatable fixture or smoke test".to_string(),
            "safety classification for each operation".to_string(),
        ],
        rollback: vec![
            "disable the draft skill by id".to_string(),
            "delete generated profile/artifacts".to_string(),
            "retain audit evidence for failure review".to_string(),
        ],
    }
}

fn parse_command(normalized: &str) -> (CommandIntent, BTreeMap<String, String>, PlannedAction) {
    let mut slots = BTreeMap::new();

    if normalized.contains("cylinder") && normalized.contains("blender") {
        slots.insert("app".to_string(), "blender".to_string());
        slots.insert("operation".to_string(), "render_cylinder".to_string());
        return (
            CommandIntent::RunAppSkill,
            slots,
            PlannedAction::RunSkill {
                skill_id: "blender.procedural".to_string(),
                operation: "render_cylinder".to_string(),
                command: Some("blender".to_string()),
                args: vec![
                    "--background".to_string(),
                    "--python".to_string(),
                    "scripts/blender-cylinder.py".to_string(),
                ],
            },
        );
    }

    if let Some(rest) = normalized.strip_prefix("open browser to ") {
        let url = normalize_url(rest);
        slots.insert("url".to_string(), url.clone());
        return (
            CommandIntent::OpenUrl,
            slots,
            PlannedAction::OpenUrl {
                command: "xdg-open".to_string(),
                args: vec![url],
            },
        );
    }

    if let Some(rest) = normalized.strip_prefix("open ") {
        let app = rest.split_whitespace().next().unwrap_or(rest).to_string();
        slots.insert("app".to_string(), app.clone());
        let (command, args) = resolve_launch_alias(&app);
        return (
            CommandIntent::OpenApp,
            slots,
            PlannedAction::LaunchApp { command, args },
        );
    }

    if let Some(window) = normalized
        .strip_prefix("focus window ")
        .and_then(parse_window_id)
    {
        slots.insert("window".to_string(), window.to_string());
        return (
            CommandIntent::FocusWindow,
            slots,
            PlannedAction::FocusWindow { window },
        );
    }

    for (phrase, slot) in [
        ("move window left", "left"),
        ("move window right", "right"),
        ("move window top", "top"),
        ("move window bottom", "bottom"),
        ("maximize window", "full"),
    ] {
        if normalized == phrase {
            slots.insert("slot".to_string(), slot.to_string());
            return (
                CommandIntent::MoveWindow,
                slots,
                PlannedAction::TileWindow {
                    window: None,
                    slot: slot.to_string(),
                },
            );
        }
    }

    if let Some(workspace) = normalized
        .strip_prefix("switch workspace ")
        .and_then(|value| value.parse::<u32>().ok())
    {
        slots.insert("workspace".to_string(), workspace.to_string());
        return (
            CommandIntent::SwitchWorkspace,
            slots,
            PlannedAction::SwitchWorkspace { workspace },
        );
    }

    (
        CommandIntent::AskClarification,
        slots,
        PlannedAction::Clarify {
            reason: "no deterministic command pattern matched".to_string(),
        },
    )
}

fn resolve_launch_alias(app: &str) -> (String, Vec<String>) {
    match app {
        "terminal" | "shell" | "console" => ("wmaker-open-terminal".to_string(), Vec::new()),
        "browser" | "web" | "brave" => ("wmaker-open-browser".to_string(), Vec::new()),
        "chrome" | "chromium" => ("wmaker-open-chrome".to_string(), Vec::new()),
        "blender" => ("wmaker-open-blender".to_string(), Vec::new()),
        "libreoffice" | "office" | "writer" => ("wmaker-open-libreoffice".to_string(), Vec::new()),
        "gimp" => ("wmaker-open-gimp".to_string(), Vec::new()),
        "inkscape" => ("wmaker-open-inkscape".to_string(), Vec::new()),
        other => (other.to_string(), Vec::new()),
    }
}

fn classify_action(action: &PlannedAction) -> SafetyDecision {
    let (risk, confirmation_required, reason) = match action {
        PlannedAction::RunSkill {
            skill_id,
            operation,
            ..
        } if skill_id == "blender.procedural" && operation == "render_cylinder" => (
            RiskLevel::LocalMutation,
            false,
            "local artifact creation in an operator-controlled path".to_string(),
        ),
        PlannedAction::LaunchApp { .. }
        | PlannedAction::OpenUrl { .. }
        | PlannedAction::FocusWindow { .. }
        | PlannedAction::TileWindow { .. }
        | PlannedAction::ResizeWindow { .. }
        | PlannedAction::SwitchWorkspace { .. } => (
            RiskLevel::LocalNavigation,
            false,
            "local desktop/navigation action".to_string(),
        ),
        PlannedAction::RunSkill { .. } => (
            RiskLevel::LocalMutation,
            true,
            "untrusted skill operation requires confirmation".to_string(),
        ),
        PlannedAction::Clarify { .. } => (
            RiskLevel::LocalNavigation,
            false,
            "clarification does not execute an action".to_string(),
        ),
    };
    SafetyDecision {
        allowed_without_confirmation: !confirmation_required,
        confirmation_required,
        risk,
        reason,
    }
}

fn normalize_command(text: &str) -> String {
    text.trim()
        .to_ascii_lowercase()
        .replace(" dot ", ".")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_url(text: &str) -> String {
    let trimmed = text.trim().replace(' ', "");
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed
    } else {
        format!("https://{trimmed}")
    }
}

fn parse_window_id(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if let Some(hex) = trimmed.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u32>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_fixture_open_browser_url_without_model() {
        let routed = route(&RouteCommandParams {
            text: "open browser to example dot com".to_string(),
            source: CommandSource::TranscriptFixture,
            confidence: Some(0.98),
            dry_run: true,
            confirmed: false,
            wait_ms: None,
        });

        assert!(matches!(routed.intent, CommandIntent::OpenUrl));
        assert_eq!(routed.slots["url"], "https://example.com");
        assert!(!routed.safety.confirmation_required);
        assert_eq!(routed.result.status, "dry_run");
    }

    #[test]
    fn routes_blender_cylinder_to_seed_skill() {
        let routed = route(&RouteCommandParams {
            text: "make a cylinder in Blender and render it".to_string(),
            source: CommandSource::Typed,
            confidence: None,
            dry_run: true,
            confirmed: false,
            wait_ms: None,
        });

        assert!(matches!(routed.intent, CommandIntent::RunAppSkill));
        assert_eq!(routed.slots["operation"], "render_cylinder");
        assert!(!routed.safety.confirmation_required);
    }

    #[test]
    fn routes_open_terminal_to_deterministic_launcher() {
        let routed = route(&RouteCommandParams {
            text: "open terminal".to_string(),
            source: CommandSource::Typed,
            confidence: None,
            dry_run: true,
            confirmed: false,
            wait_ms: None,
        });

        assert!(matches!(routed.intent, CommandIntent::OpenApp));
        assert_eq!(routed.slots["app"], "terminal");
        match routed.action {
            PlannedAction::LaunchApp { command, args } => {
                assert_eq!(command, "wmaker-open-terminal");
                assert!(args.is_empty());
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn unknown_command_clarifies_instead_of_guessing() {
        let routed = route(&RouteCommandParams {
            text: "do the thing".to_string(),
            source: CommandSource::Typed,
            confidence: None,
            dry_run: false,
            confirmed: false,
            wait_ms: None,
        });

        assert!(matches!(routed.intent, CommandIntent::AskClarification));
        assert!(!routed.result.executed);
        assert!(!routed.result.ok);
    }

    #[test]
    fn registry_contains_seed_browser_and_blender_skills() {
        let registry = skills(false);
        assert!(
            registry
                .skills
                .iter()
                .any(|skill| skill.id == "browser.managed")
        );
        assert!(
            registry
                .skills
                .iter()
                .any(|skill| skill.id == "blender.procedural")
        );
    }
}
