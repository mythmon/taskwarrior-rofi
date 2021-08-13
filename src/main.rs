use anyhow::{anyhow, bail, Context, Result};
use chrono::{offset::Local as LocalTime, NaiveDateTime};
use rofi::Rofi;
use std::{
    fmt::Display,
    process::{Command, Stdio},
};
use task_hookrs::{
    annotation::Annotation, date::Date as TwDate, status::TaskStatus, task::Task, tw,
};

fn main() {
    match ui() {
        Ok(_) => {}
        Err(err) => match err.downcast_ref::<rofi::Error>() {
            Some(rofi::Error::Interrupted) => (),
            Some(_) | None => {
                Rofi::new(&vec![format!("Error: {}", err)])
                    .run()
                    .expect("Couldn't even use rofi to show an error");
            }
        },
    }
}

fn ui() -> Result<()> {
    loop {
        let actions = Action::all();
        let action = rich_rofi("Choose an action", actions)?;

        match action {
            Action::Add => {
                let (task_text, annotations) = {
                    let input = Rofi::<String>::new(&vec![])
                        .prompt("task -- annotation")
                        .run()?;
                    let mut parts = input.split("--");
                    (
                        parts
                            .next()
                            .ok_or_else(|| anyhow!("No input given to add"))?
                            .to_string(),
                        parts.map(|ann| ann.trim().to_string()).collect::<Vec<_>>(),
                    )
                };

                add_task(task_text, annotations)?;
            }

            Action::List => {
                match task_rofi("Press enter to go back") {
                    Ok(_) => Ok(()),
                    Err(e) => match e.downcast_ref::<rofi::Error>() {
                        Some(rofi::Error::Interrupted) => Ok(()),
                        _ => Err(e),
                    },
                }?;
            }

            Action::Mod => {
                let mut task = task_rofi("Choose a task")?;
                mod_task(&mut task)?
            }

            Action::Exit => return Ok(()),

            _ => {
                let mut task = task_rofi("Choose a task")?;
                match action {
                    Action::Done => *task.status_mut() = TaskStatus::Completed,
                    Action::Start => task.set_start(Some(LocalTime::now().naive_local())),
                    Action::Stop => task.set_start::<NaiveDateTime>(None),
                    Action::Delete => *task.status_mut() = TaskStatus::Deleted,
                    Action::Open => {
                        task.open_annotation()?;
                        break;
                    }

                    Action::Annotate => {
                        let input = Rofi::<String>::new(&vec![]).prompt("annotation").run()?;
                        let annotation =
                            Annotation::new(LocalTime::now().naive_local().into(), input);
                        match task.annotations_mut() {
                            Some(annotations) => annotations.push(annotation),
                            None => {
                                task.set_annotations::<Vec<_>, Annotation>(Some(vec![annotation]))
                            }
                        }
                    }

                    Action::Wait => {
                        let input = Rofi::<String>::new(&vec![
                            "tomorrow".to_string(),
                            "1h".to_string(),
                            "2h".to_string(),
                            "4h".to_string(),
                            "monday".to_string(),
                        ])
                        .prompt("Wait until?")
                        .run()?;

                        task_command(vec![
                            &task.uuid().to_string(),
                            "mod",
                            &format!("wait:{}", input),
                        ])
                        .context("modifying wait")?;
                    }

                    Action::Mod | Action::Add | Action::List | Action::Exit => {
                        unreachable!("Already handled this case")
                    }
                }
                tw::save(Some(&task)).map_failure()?;
            }
        }
    }
    Ok(())
}

fn task_rofi(prompt: &str) -> Result<Task> {
    let default_command = get_config_var("default.command")?;
    let default_filter = get_config_var(&format!("report.{}.filter", default_command))?;
    let mut tasks = tw::query(&default_filter).unwrap();
    tasks.sort_unstable_by_key(|task| task.urgency().map(|u| (-u * 10_000f64) as i32));
    let labeled_tasks: Vec<_> = tasks
        .into_iter()
        .map(|task| LabeledItem {
            label: format_task(&task),
            item: task,
        })
        .collect();
    Ok(rich_rofi(prompt, labeled_tasks)?)
}

fn get_config_var(name: &str) -> Result<String> {
    task_command(vec!["show", name])?
        .0
        .lines()
        .filter_map(|line| {
            let parts: Vec<_> = line.split(' ').collect();
            if parts.len() > 1 && parts[0] == name {
                Some(parts[1..].join(" "))
            } else {
                None
            }
        })
        .next()
        .ok_or_else(|| anyhow!("Could not find default command"))
}

fn add_task(task_text: String, new_annotations: Vec<String>) -> Result<()> {
    let mut args = vec!["add"];
    args.extend(task_text.split_whitespace());
    let (stdout, stderr) = task_command(args).context("adding task")?;

    if !stdout.starts_with("Created task ") {
        bail!(
            "Unexpected output from add command: `{}` / stderr: `{}`",
            stdout,
            stderr
        );
    }
    let task_id = stdout.split_whitespace().last().unwrap();

    if !new_annotations.is_empty() {
        let now: TwDate = LocalTime::now().naive_local().into();
        let new_annotations = new_annotations
            .iter()
            .map(|ann| Annotation::new(now.clone(), ann.to_string()))
            .collect::<Vec<_>>();

        let mut tasks = tw::query(task_id).map_failure()?;
        if tasks.len() != 1 {
            bail!("Querying by ID should return exactly one task");
        }
        let task = &mut tasks[0];

        match task.annotations_mut() {
            Some(annotations) => {
                annotations.extend(new_annotations);
            }
            None => task.set_annotations::<Vec<_>, Annotation>(Some(new_annotations)),
        };
        tw::save(Some(&*task))
            .map_err(|err| anyhow!("tw error: {}", err))
            .context("Failed to save annotations")?;
    }

    Ok(())
}

fn mod_task(task: &mut Task) -> Result<()> {
    let task_id = task
        .id()
        .map(|id| id.to_string())
        .unwrap_or_else(|| task.uuid().to_string());
    let input = Rofi::<String>::new(&vec![])
        .prompt(format!("Mods for task {}", task_id))
        .run()?;

    let mut args: Vec<&str> = vec![&task_id, "mod"];
    args.extend(input.split_whitespace());
    task_command(args).context("modifying task")?;

    Ok(())
}

enum Action {
    Add,
    Delete,
    Done,
    List,
    Start,
    Stop,
    Open,
    Mod,
    Wait,
    Annotate,
    Exit,
}

impl Action {
    fn all() -> Vec<Self> {
        vec![
            Self::List,
            Self::Add,
            Self::Done,
            Self::Start,
            Self::Stop,
            Self::Delete,
            Self::Open,
            Self::Mod,
            Self::Wait,
            Self::Annotate,
            Self::Exit,
        ]
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Action::Add => "Add",
                Action::Delete => "Delete",
                Action::Done => "Done",
                Action::List => "List",
                Action::Start => "Start",
                Action::Stop => "Stop",
                Action::Open => "Open",
                Action::Mod => "Mod",
                Action::Wait => "Wait",
                Action::Annotate => "Annotate",
                Action::Exit => "Exit (Escape)",
            }
        )
    }
}

fn format_task(task: &Task) -> String {
    let mut parts = vec![];
    let max_desc = 60;

    if let Some(id) = task.id() {
        parts.push(format!("[{:>2}]", id));
    } else {
        parts.push("[--]".to_string());
    }

    if task.description().len() <= max_desc {
        parts.push(format!("{:<width$}", task.description(), width = max_desc));
    } else {
        let truncated = &task.description()[..max_desc - 3];
        parts.push(format!("{}...", truncated));
    }

    if let Some(urgency) = task.urgency() {
        parts.push(format!("(u={:+.2})", urgency));
    }

    if let Some(project) = task.project() {
        parts.push(format!("proj:{}", project));
    }

    parts.join(" ")
}

struct LabeledItem<T> {
    label: String,
    item: T,
}

impl<T> From<T> for LabeledItem<T>
where
    T: Display,
{
    fn from(item: T) -> Self {
        Self {
            label: item.to_string(),
            item,
        }
    }
}

impl<T> Display for LabeledItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

fn rich_rofi<T, U>(prompt: &str, items: Vec<T>) -> Result<U, rofi::Error>
where
    T: Into<LabeledItem<U>>,
{
    let mut items: Vec<LabeledItem<U>> = items.into_iter().map(|i| i.into()).collect();
    let labels = items.iter().map(|i| &i.label).collect();
    let idx = Rofi::new(&labels).prompt(prompt).run_index()?;
    // use `swap_remove` so we don't have to re-order the list we're about the throw away anyways
    Ok(items.swap_remove(idx).item)
}

trait TaskExt {
    fn open_annotation(&self) -> Result<()>;
}

impl TaskExt for Task {
    fn open_annotation(&self) -> Result<()> {
        let annotations = self
            .annotations()
            .ok_or_else(|| anyhow!("No annotations found"))?;
        let with_links: Vec<_> = annotations
            .iter()
            .filter(|ann| {
                ann.description().starts_with("https://")
                    || ann.description().starts_with("http://")
            })
            .collect();

        let choice: &Annotation = match with_links.len() {
            0 => bail!("No annotation links found"),
            1 => with_links[0],
            _ => {
                let mut labeled: Vec<_> = with_links
                    .into_iter()
                    .map(|ann| LabeledItem {
                        label: format!("{} {}", ann.entry().format("%Y-%m-%d"), ann.description()),
                        item: ann,
                    })
                    .collect();
                labeled.sort_by(|a, b| a.label.cmp(&b.label).reverse());

                rich_rofi("Choose annotation", labeled).context("Couldn't choose an annotation")?
            }
        };

        open::that(choice.description()).context("Could not open item specified by annotation")?;

        Ok(())
    }
}

trait MapFailure {
    type MappedError;

    fn map_failure(self) -> Self::MappedError;
}

impl<T> MapFailure for Result<T, failure::Error> {
    type MappedError = Result<T, anyhow::Error>;

    fn map_failure(self) -> Self::MappedError {
        self.map_err(|err| anyhow!("tw error: {}", err))
    }
}

fn task_command(args: Vec<&str>) -> Result<(String, String)> {
    let result = Command::new("task")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(args)
        .spawn()?
        .wait_with_output()?;

    let stdout = String::from_utf8(result.stdout)?;
    let stderr = String::from_utf8(result.stderr)?;

    if !result.status.success() {
        bail!("stdout: {} / stderr: {}", stdout, stderr);
    }

    Ok((stdout, stderr))
}
