//! MCP prompts (paper §IV-F).
//!
//! Server-rendered prompt templates that the MCP client can instantiate
//! with arguments.  Each prompt encapsulates an Oracle-specific workflow that
//! the model would otherwise have to compose from scratch.

use mcp_core::{GetPromptResult, Prompt, PromptArgument, PromptMessage, Role, ToolContent};
use mcp_server::{registry::PromptHandler, PromptDescriptor};
use oracle_automate_skills::{Skill, SkillRegistry};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub fn all(skill_registry: &SkillRegistry) -> Vec<PromptDescriptor> {
    let mut out = vec![
        review_rfc_call(),
        sandbox_impact_analysis(),
        review_where_used(),
    ];
    // marianfoo/sap-ai-mcp-servers pattern: skills auto-loaded from disk
    // become MCP prompts.  Each markdown file = one prompt.
    for skill in skill_registry.skills() {
        out.push(skill_as_prompt(skill.clone()));
    }
    out
}

fn skill_as_prompt(skill: Skill) -> PromptDescriptor {
    let skill_for_handler = skill.clone();
    struct H(Skill);
    impl PromptHandler for H {
        fn get(
            &self,
            arguments: Option<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<GetPromptResult>> + Send + '_>> {
            let skill = self.0.clone();
            Box::pin(async move {
                let arg_map: HashMap<String, String> = match arguments {
                    Some(serde_json::Value::Object(m)) => m
                        .into_iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
                        .collect(),
                    _ => HashMap::new(),
                };
                let body = skill.render(&arg_map);
                Ok(GetPromptResult {
                    description: Some(skill.description.clone()),
                    messages: vec![PromptMessage {
                        role: Role::User,
                        content: ToolContent::text(body),
                    }],
                })
            })
        }
    }
    PromptDescriptor {
        prompt: Prompt {
            name: skill_for_handler.name.clone(),
            description: Some(skill_for_handler.description.clone()),
            arguments: skill_for_handler
                .arguments
                .iter()
                .map(|a| PromptArgument {
                    name: a.name.clone(),
                    description: a.description.clone(),
                    required: a.required,
                })
                .collect(),
        },
        handler: Arc::new(H(skill_for_handler)),
    }
}

fn review_where_used() -> PromptDescriptor {
    struct H;
    impl PromptHandler for H {
        fn get(
            &self,
            arguments: Option<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<GetPromptResult>> + Send + '_>> {
            let args = arguments.unwrap_or(serde_json::Value::Object(Default::default()));
            let object = args
                .get("object")
                .and_then(|v| v.as_str())
                .unwrap_or("<OBJECT>")
                .to_string();
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("Class")
                .to_string();
            Box::pin(async move {
                let body = format!(
                    "Before changing or deleting {kind} {object}, run oracle.oic.where_used and reason carefully about the impact.\n\nSteps:\n1. Call oracle.oic.where_used with name={object}, kind={} to enumerate every invoker / dependent / reference site.\n2. For each hit, group by ownership (project, package) using oracle.oic.get_project_contents on the parent.\n3. Identify which of those dependents are themselves on a hot path (use oracle.docs.search to cross-reference business processes).\n4. Produce a 3-section report: Direct dependents, Indirect dependents, Recommended pre-change checks (regression tests, sandboxes to coordinate).\n\nCite every claim by its source URI (oracle-rest://, oracle-object://, or oracle-help://).",
                    kind.to_lowercase(),
                );
                Ok(GetPromptResult {
                    description: Some(
                        "Where-used review before changing or deleting an Oracle artifact.".into(),
                    ),
                    messages: vec![PromptMessage {
                        role: Role::User,
                        content: ToolContent::text(body),
                    }],
                })
            })
        }
    }
    PromptDescriptor {
        prompt: Prompt {
            name: "oracle.review-where-used".into(),
            description: Some(
                "Walk the agent through a where-used analysis before changing an Oracle artifact."
                    .into(),
            ),
            arguments: vec![
                PromptArgument {
                    name: "object".into(),
                    description: Some("Object name".into()),
                    required: true,
                },
                PromptArgument {
                    name: "kind".into(),
                    description: Some(
                        "Artifact kind (integration | groovy_script | connection | ...)".into(),
                    ),
                    required: false,
                },
            ],
        },
        handler: Arc::new(H),
    }
}

fn review_rfc_call() -> PromptDescriptor {
    struct H;
    impl PromptHandler for H {
        fn get(
            &self,
            arguments: Option<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<GetPromptResult>> + Send + '_>> {
            let args = arguments.unwrap_or(serde_json::Value::Object(Default::default()));
            let function = args
                .get("function")
                .and_then(|v| v.as_str())
                .unwrap_or("<UNKNOWN>")
                .to_string();
            let parameters = args
                .get("parameters")
                .cloned()
                .unwrap_or(serde_json::Value::Object(Default::default()));
            Box::pin(async move {
                let body = format!(
                    "Review the following proposed Oracle REST call before execution. Confirm it is the right function for the user's intent, that every required parameter is present and well-typed, that the parameter values are realistic for the target environment, and that the side-effects are acceptable. Cite the source for each claim.\n\nFunction: {function}\nParameters:\n{}\n\nIf safe, summarise what the call will do, the affected tables, and the user-visible result. If unsafe, identify the specific risk and propose a safer alternative.",
                    serde_json::to_string_pretty(&parameters).unwrap_or_default(),
                );
                Ok(GetPromptResult {
                    description: Some("Pre-execution review of a proposed oracle.rest.call".into()),
                    messages: vec![PromptMessage {
                        role: Role::User,
                        content: ToolContent::text(body),
                    }],
                })
            })
        }
    }
    PromptDescriptor {
        prompt: Prompt {
            name: "oracle.review-rest-call".into(),
            description: Some(
                "Pre-flight review of a proposed oracle.rest.call invocation.".into(),
            ),
            arguments: vec![
                PromptArgument {
                    name: "function".into(),
                    description: Some("REST operation name".into()),
                    required: true,
                },
                PromptArgument {
                    name: "parameters".into(),
                    description: Some("Parameters object".into()),
                    required: false,
                },
            ],
        },
        handler: Arc::new(H),
    }
}

fn sandbox_impact_analysis() -> PromptDescriptor {
    struct H;
    impl PromptHandler for H {
        fn get(
            &self,
            arguments: Option<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = mcp_core::Result<GetPromptResult>> + Send + '_>> {
            let args = arguments.unwrap_or(serde_json::Value::Object(Default::default()));
            let sandbox = args
                .get("sandbox")
                .and_then(|v| v.as_str())
                .unwrap_or("<SANDBOX>")
                .to_string();
            let scope = args
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("PRODUCTION")
                .to_string();
            Box::pin(async move {
                let body = format!(
                    "Analyse the impact of publishing configuration sandbox {sandbox} to the {scope} pod.\n\nSteps:\n1. Use oracle.docs.search to find related Oracle Help Center content for the objects the sandbox touches.\n2. Use oracle.oic.where_used / oracle.rest.search to find integrations and REST resources that reference the changed artifacts.\n3. Use oracle.object.read on FND_SANDBOXES to enumerate the sandbox's contents and status.\n4. Use eam.search_apps to enumerate downstream applications in the portfolio.\n5. Produce a 3-section report: Direct impact, Indirect impact, Recommended pre-publish checks (validation run, dependent sandboxes, FSM configuration-package coordination).\n\nCite every claim by its source URI.",
                );
                Ok(GetPromptResult {
                    description: Some(
                        "Cross-domain impact analysis before publishing a configuration sandbox"
                            .into(),
                    ),
                    messages: vec![PromptMessage {
                        role: Role::User,
                        content: ToolContent::text(body),
                    }],
                })
            })
        }
    }
    PromptDescriptor {
        prompt: Prompt {
            name: "oracle.sandbox-impact-analysis".into(),
            description: Some("Multi-tool cross-domain impact analysis before publishing a configuration sandbox.".into()),
            arguments: vec![
                PromptArgument { name: "sandbox".into(), description: Some("Sandbox name (e.g. GT_AR_AUTOINVOICE_FIX)".into()), required: true },
                PromptArgument { name: "scope".into(), description: Some("Target pod (PRODUCTION / QA / DEV)".into()), required: false },
            ],
        },
        handler: Arc::new(H),
    }
}
