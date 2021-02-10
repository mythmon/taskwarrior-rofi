use std::{fmt::Display, process::Command};

use chrono::{offset::Local as LocalTime, NaiveDateTime};
use rofi::Rofi;
use task_hookrs::{annotation::Annotation, status::TaskStatus, task::Task, tw};

fn main() {
    if let Result::Err(e) = ui() {
        match Rofi::new(&vec![format!("{}", e)]).run() {
            Err(rofi::Error::IoError(err)) => {
                println!("Error: {}", err);
            }
            Ok(_) | Err(_) => {}
        }
    }
}

fn ui() -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let actions = Action::all();
        let action = rich_rofi("Choose an action", actions)?;

        match action {
            Action::Add => {
                let task_text = Rofi::<String>::new(&vec![])
                    .prompt("Task description")
                    .run()?;
                let rv = {
                    let mut command = Command::new("task");
                    command.arg("add");
                    for word in task_text.split_whitespace() {
                        command.arg(word);
                    }
                    command.spawn()?.wait_with_output()?
                };
                if !rv.status.success() {
                    let stdout = String::from_utf8(rv.stdout)?;
                    let stderr = String::from_utf8(rv.stderr)?;
                    return Err(format!("{}\n{}", stdout, stderr).into());
                }
                break;
            }
            Action::List => {
                match task_rofi("Press enter to go back") {
                    Ok(_) => Ok(()),
                    Err(rofi::Error::Interrupted) => Ok(()),
                    Err(e) => Err(e)
                }?;
            }
            _ => {
                let mut task = task_rofi("Choose a task")?;
                match action {
                    Action::Done => *task.status_mut() = TaskStatus::Completed,
                    Action::Start => task.set_start(Some(LocalTime::now().naive_local())),
                    Action::Stop => task.set_start::<NaiveDateTime>(None),
                    Action::Delete => *task.status_mut() = TaskStatus::Deleted,
                    Action::Open => task.open_annotation()?,
                    Action::Add | Action::List => unreachable!("Already handled this case"),
                }
                tw::save(Some(&task))?;
                break;
            }
        }
    }
    Ok(())
}

fn task_rofi(prompt: &str) -> Result<Task, rofi::Error> {
    let mut tasks = tw::query("status:pending").unwrap();
    tasks.sort_unstable_by_key(|task| task.urgency().map(|u| (-u * 10_000f64) as u64));
    let labeled_tasks: Vec<_> = tasks
        .into_iter()
        .map(|task| LabeledItem {
            label: format_task(&task),
            item: task,
        })
        .collect();
    rich_rofi(prompt, labeled_tasks)
}

enum Action {
    Add,
    Delete,
    Done,
    List,
    Start,
    Stop,
    Open,
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
    fn open_annotation(&self) -> Result<(), String>;
}

impl TaskExt for Task {
    fn open_annotation(&self) -> Result<(), String> {
        let annotations = self.annotations().ok_or("No annotations found")?;
        let with_links: Vec<_> = annotations
            .iter()
            .filter(|ann| {
                ann.description().starts_with("https://")
                    || ann.description().starts_with("http://")
            })
            .collect();

        let choice: &Annotation = match with_links.len() {
            0 => return Err("No annotation links found".to_string()),
            1 => with_links[0],
            _ => {
                return Err("Too many links found".to_string());
                // TODO rofi to pick one
            }
        };

        open::that(choice.description()).map_err(|err| err.to_string())?;

        Ok(())
    }
}
