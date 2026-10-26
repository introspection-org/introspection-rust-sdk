//! Steps 14 to 16: a trial graded by the recipe's judge, an experiment over
//! two versions, commands in the trial's sandbox, and an automation.

use std::time::Duration;

use futures::StreamExt as _;
use introspection_sdk::{
    AutomationCreateParams, AutomationListParams, AutomationTriggerType, AutomationUpdateParams,
    EventListParams, ExperimentListParams, ImageListParams, IntrospectionEventName,
    JudgeListParams, JudgeUpdateParams, PaginationParams, RunRequest, RunnerIdentity, Task,
    TaskCommand, TaskCommandListParams, TaskCommandSpec, TaskCommandStatus, TaskCreate, TaskKind,
    TaskStatus,
};
use serde_json::{json, Value};

use crate::support::{assert_stable, settled, walk};
use crate::{set, Walk};

/// What eval compose services run as, the org's provider installs, and the
/// recipe's judge.
async fn judge(w: &Walk) -> introspection_sdk::Judge {
    let sa = w.sa();
    let project = w.project_id();
    let group = set(&w.runtime, "the runtime")
        .runtime_group_id
        .expect("a runtime group");
    let mut images = sa.images().list(&ImageListParams::default());
    let images = images
        .next_page()
        .await
        .expect("list images")
        .expect("a page");
    if let Some(image) = images.records.first() {
        sa.images().get(image.id).await.expect("read an image");
    }
    w.owner()
        .integrations()
        .list(&PaginationParams::default())
        .collect_all(0)
        .await
        .expect("list integrations");
    let judges = sa
        .judges()
        .list(&JudgeListParams {
            project: Some(project.into()),
            runtime_group_id: Some(group),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list judges");
    let judge = judges
        .into_iter()
        .find(|j| j.name == "greeting")
        .expect("the recipe's judge is synced");
    let read = sa
        .judges()
        .get(judge.id, Some(project.into()))
        .await
        .expect("read the judge");
    assert_eq!(read.name, "greeting");
    sa.judges()
        .update(
            judge.id,
            &JudgeUpdateParams {
                enabled: Some(true),
                sample_rate: Some(1.0),
            },
            Some(project.into()),
        )
        .await
        .expect("enable the judge");
    judge
}

pub async fn eval(w: &mut Walk) {
    let sa = w.sa();
    let quiet = w.sa_quiet();
    let runtime = set(&w.runtime, "the runtime");
    let judge = judge(w).await;

    // A trial needs the evals capability, which the runner's session never
    // carries; the service account holds `tasks:eval` and names the runtime.
    let created = sa
        .tasks()
        .create(&TaskCreate {
            kind: Some(TaskKind::Eval),
            runtime_id: Some(runtime.id),
            commands: Some(true),
            prompt: Some(w.settings.prompt()),
            agent_name: Some(w.settings.agent.clone()),
            metadata: Some([("suite".to_string(), json!("walk"))].into()),
            idle_timeout_seconds: Some(120),
            ..Default::default()
        })
        .await
        .expect("create the trial");
    let trial = created.task;
    assert_eq!(
        trial.metadata.as_ref().and_then(|m| m.get("suite")),
        Some(&json!("walk"))
    );
    assert_stable!(trial, { ".metadata" => "[platform's]" }, @r#"
    {
      "agent": {
        "sandbox_status": "provisioning"
      },
      "created_at": "[timestamp]",
      "display_index": "[number]",
      "id": "[uuid]",
      "is_archived": false,
      "kind": "eval",
      "last_user_message_at": "[timestamp]",
      "member_id": "[uuid]",
      "metadata": "[platform's]",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "runtime_group_id": "[uuid]",
      "runtime_id": "[uuid]",
      "status": "pending",
      "tags": [],
      "title": "Call the `echo` tool from the declared `hello-mcp` MCP server with the message `hello from the public MCP`. Write the echoed value exactly, with no quotes or additional formatting, to `/workspace/outputs/greeting.txt`. Then run `echo $WALK_MARKER` in…",
      "updated_at": "[timestamp]"
    }
    "#);
    // Read the run's stream to its end, then let the row settle.
    let id = trial.id.to_string();
    let stream = sa
        .tasks()
        .runs
        .stream(&id, &created.run.id)
        .await
        .expect("the trial's stream");
    let mut stream = Box::pin(stream);
    while let Some(event) = stream.next().await {
        event.expect("a run event");
    }
    let tasks = sa.tasks();
    let quiet_tasks = quiet.tasks();
    let trial = settled(
        w.mode,
        "the trial",
        Duration::from_secs(180),
        || tasks.get(&id),
        || quiet_tasks.get(&id),
        |t: &Task| {
            matches!(
                t.status,
                TaskStatus::Idle
                    | TaskStatus::AwaitingUser
                    | TaskStatus::Completed
                    | TaskStatus::Failed
            )
        },
    )
    .await;
    assert!(matches!(
        trial.status,
        TaskStatus::Idle | TaskStatus::AwaitingUser | TaskStatus::Completed
    ));
    w.eval_task = Some(trial);
    w.judge = Some(judge);
    experiment(w).await;
}

fn experiment_document(w: &Walk, name: &str) -> Value {
    let runtime = set(&w.runtime, "the runtime");
    json!({
        "name": name,
        "project": w.project_id(),
        "runtime_group_id": runtime.runtime_group_id,
        "goal_json": {
            "kind": "composite",
            "components": [{ "source": "judge", "judge_id": set(&w.judge, "the judge").id, "weight": 1 }],
        },
        "arms": [
            { "runtime_id": runtime.id, "arm_label": "v1" },
            { "runtime_id": set(&w.runtime_v2, "the second version").id, "arm_label": "v2" },
        ],
    })
}

async fn experiment(w: &mut Walk) {
    let sa = w.sa();
    let project = w.project_id();
    let experiment = sa
        .experiments()
        .create(&experiment_document(w, walk::EXPERIMENT_NAMES[0]))
        .await
        .expect("create the experiment");
    assert_stable!(experiment, @r#"
    {
      "arms": [
        {
          "arm_label": "v1",
          "id": "[uuid]",
          "initial_weight": 50,
          "runtime_id": "[uuid]"
        },
        {
          "arm_label": "v2",
          "id": "[uuid]",
          "initial_weight": 50,
          "runtime_id": "[uuid]"
        }
      ],
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "environment": "production",
      "goal_json": {
        "components": [
          {
            "judge_definition_hash": "[digest]",
            "judge_id": "[uuid]",
            "source": "judge",
            "weight": 1.0
          }
        ],
        "direction": "maximize",
        "kind": "composite"
      },
      "hash_key_fields": [
        "user.id",
        "anonymous.id",
        "conversation.id"
      ],
      "id": "[uuid]",
      "name": "The walk's experiment",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "routing_strategy": "beta_sample",
      "runtime_group_id": "[uuid]",
      "sample_rate": 1.0,
      "scoring_interval_seconds": 300,
      "status": "draft",
      "updated_at": "[timestamp]"
    }
    "#);
    sa.experiments()
        .get(experiment.id, project)
        .await
        .expect("read the experiment");
    sa.experiments()
        .update(
            experiment.id,
            project,
            &json!({ "description": "two versions, one judge" }),
        )
        .await
        .expect("describe the experiment");
    let handle = sa.experiment(experiment.id, project);
    let started = handle.start().await.expect("start the experiment");
    assert_eq!(started.status.as_str(), "running");
    let listed = sa
        .experiments()
        .list(&ExperimentListParams {
            project: Some(project.into()),
            ..Default::default()
        })
        .collect_all(0)
        .await
        .expect("list experiments");
    assert!(listed.iter().any(|e| e.id == experiment.id));
    // Bandit routing needs a stable subject to assign an arm to.
    handle
        .run(RunRequest {
            identity: Some(RunnerIdentity {
                user_id: Some("sdk-walk-user".into()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .expect("a runner on the experiment");
    let ended = handle.end().await.expect("end the experiment");
    assert_stable!(ended, @r#"
    {
      "arms": [
        {
          "arm_label": "v1",
          "id": "[uuid]",
          "initial_weight": 50,
          "runtime_id": "[uuid]"
        },
        {
          "arm_label": "v2",
          "id": "[uuid]",
          "initial_weight": 50,
          "runtime_id": "[uuid]"
        }
      ],
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "description": "two versions, one judge",
      "ended_at": "[timestamp]",
      "environment": "production",
      "goal_json": {
        "components": [
          {
            "judge_definition_hash": "[digest]",
            "judge_id": "[uuid]",
            "source": "judge",
            "weight": 1.0
          }
        ],
        "direction": "maximize",
        "kind": "composite"
      },
      "hash_key_fields": [
        "user.id",
        "anonymous.id",
        "conversation.id"
      ],
      "id": "[uuid]",
      "name": "The walk's experiment",
      "org_id": "[uuid]",
      "project_id": "[uuid]",
      "routing_strategy": "beta_sample",
      "runtime_group_id": "[uuid]",
      "sample_rate": 1.0,
      "scoring_interval_seconds": 300,
      "started_at": "[timestamp]",
      "status": "ended",
      "updated_at": "[timestamp]"
    }
    "#);
    cancelled_experiment(w).await;
}

/// A second experiment, started and cancelled, then deleted.
async fn cancelled_experiment(w: &Walk) {
    let sa = w.sa();
    let project = w.project_id();
    let second = sa
        .experiments()
        .create(&experiment_document(w, walk::EXPERIMENT_NAMES[1]))
        .await
        .expect("create a second experiment");
    let second_handle = sa.experiment(second.id, project);
    second_handle
        .start()
        .await
        .expect("start the second experiment");
    let cancelled = second_handle
        .cancel()
        .await
        .expect("cancel the second experiment");
    assert_eq!(cancelled.status.as_str(), "cancelled");
    w.owner()
        .experiments()
        .delete(second.id, project)
        .await
        .expect("delete the cancelled experiment");
}

fn finished(c: &TaskCommand) -> bool {
    matches!(
        c.status,
        TaskCommandStatus::Completed | TaskCommandStatus::Failed
    ) || matches!(c.status.as_str(), "succeeded" | "cancelled")
}

async fn command(w: &Walk, spec: TaskCommandSpec) -> TaskCommand {
    let sa = w.sa();
    let quiet = w.sa_quiet();
    let task = set(&w.eval_task, "the trial").id.to_string();
    let created = sa
        .tasks()
        .commands
        .create(&task, &spec)
        .await
        .expect("create a command");
    let id = created.id.to_string();
    let (commands, quiet_commands) = (sa.tasks().commands, quiet.tasks().commands);
    settled(
        w.mode,
        "task command",
        Duration::from_secs(120),
        || commands.get(&task, &id, None),
        || quiet_commands.get(&task, &id, None),
        finished,
    )
    .await
}

pub async fn commands(w: &mut Walk) {
    let sa = w.sa();
    let output = format!("/workspace/outputs/{}", walk::OUTPUT_FILE_NAME);
    let exec = command(
        w,
        TaskCommandSpec::Exec {
            command: vec!["cat".into(), output.clone()],
            cwd: None,
            timeout_seconds: Some(30),
            service: None,
        },
    )
    .await;
    assert_stable!(exec, @r#"
    {
      "created_at": "[timestamp]",
      "exit_code": 0,
      "finished_at": "[timestamp]",
      "id": "[uuid]",
      "kind": "exec",
      "spec": {
        "command": [
          "cat",
          "/workspace/outputs/greeting.txt"
        ],
        "cwd": null,
        "kind": "exec",
        "service": null,
        "timeout_seconds": 30
      },
      "started_at": "[timestamp]",
      "status": "completed",
      "stderr": "",
      "stdout": "Echo: hello from the public MCP",
      "task_id": "[uuid]",
      "updated_at": "[timestamp]"
    }
    "#);
    assert_eq!(
        exec.stdout.as_deref().map(str::trim),
        Some(walk::ECHO_RESULT)
    );
    let download = command(
        w,
        TaskCommandSpec::Download {
            path: output,
            service: None,
        },
    )
    .await;
    assert!(
        download.output_file_id.is_some(),
        "the download produced a file"
    );
    let task = set(&w.eval_task, "the trial").id.to_string();
    let listed = sa
        .tasks()
        .commands
        .list(&task, &TaskCommandListParams::default())
        .collect_all(0)
        .await
        .expect("list commands");
    assert!(listed.len() >= 2);

    // Judging runs in the sandbox at teardown and calls the judge's grader
    // through the egress. On the scripted target that grader is the model
    // double, which returns no verdict, so a judgement is asserted only
    // where a real model grades (design doc §10).
    // todo: no `judge_id` on `EventListParams`.
    let params = EventListParams {
        filters: Some([("judge_id".to_string(), json!(set(&w.judge, "the judge").id))].into()),
        ..EventListParams::new(IntrospectionEventName::Judgement)
    };
    let judgements = sa
        .events()
        .list(&params)
        .expect("an events paginator")
        .collect_all(0)
        .await
        .expect("list judgements");
    if w.settings.agent != "scripted" {
        assert!(
            !judgements.is_empty(),
            "the recipe's judge graded the trial"
        );
    }
}

pub async fn automations(w: &mut Walk) {
    let sa = w.sa();
    let automation = sa
        .automations()
        .create(&AutomationCreateParams {
            name: walk::AUTOMATION_NAME.into(),
            trigger_type: AutomationTriggerType::Manual,
            prompt: Some(w.settings.second_turn()),
            runtime_group_id: set(&w.runtime, "the runtime").runtime_group_id,
            // An automation its owner creates posts into that owner's own
            // task; the first task was archived and unarchived, which the
            // trigger still refuses, so the trial is the one it posts into.
            task_id: Some(set(&w.eval_task, "the trial").id),
            description: None,
            cron_schedule: None,
            kind: None,
            next_trigger_at: None,
            metadata: None,
            enabled: None,
        })
        .await
        .expect("create the automation");
    assert_stable!(automation, @r#"
    {
      "can_manage": true,
      "created_at": "[timestamp]",
      "created_by_member_id": "[uuid]",
      "enabled": true,
      "id": "[uuid]",
      "name": "The walk's automation",
      "org_id": "[uuid]",
      "owner_role": "operator",
      "project_id": "[uuid]",
      "prompt": "Reply with the single word done. <<SCRIPT {\"reply\":\"done\"}>>",
      "runtime_group_id": "[uuid]",
      "tags": [],
      "task_id": "[uuid]",
      "trigger_type": "manual",
      "updated_at": "[timestamp]"
    }
    "#);
    let triggered = sa
        .automations()
        .trigger(automation.id)
        .await
        .expect("trigger the automation");
    assert_stable!(triggered, @r#"
    {
      "automation_id": "[uuid]",
      "status": "triggered",
      "task_id": "[uuid]"
    }
    "#);
    sa.automations()
        .get(automation.id)
        .await
        .expect("read the automation");
    sa.automations()
        .update(
            automation.id,
            &AutomationUpdateParams {
                description: Some("triggered once by the walk".into()),
                ..Default::default()
            },
        )
        .await
        .expect("describe the automation");
    let listed = sa
        .automations()
        .list(&AutomationListParams::default())
        .collect_all(0)
        .await
        .expect("list automations");
    assert!(listed.iter().any(|a| a.id == automation.id));
}
