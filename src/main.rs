use std::{fmt::Display, process::Command};

use chrono::{offset::Local as LocalTime, NaiveDateTime};
use rofi::Rofi;
use task_hookrs::{status::TaskStatus, task::Task, tw};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let actions = Action::all();
        let action = rich_rofi("Choose an action", actions)?;

        match action {
            Action::Add => {
                let task_text = Rofi::<String>::new(&vec![])
                    .prompt("Task description")
                    .run()?;
                let rv = Command::new("task")
                    .arg("add")
                    .arg(task_text)
                    .spawn()?
                    .wait_with_output()?;
                if !rv.status.success() {
                    let stdout = String::from_utf8(rv.stdout)?;
                    let stderr = String::from_utf8(rv.stderr)?;
                    return Err(format!("{}\n{}", stdout, stderr).into());
                }
                break;
            }
            Action::List => {
                let tasks = tw::query("status:pending").unwrap();
                let labeled_tasks: Vec<_> = tasks
                    .into_iter()
                    .map(|task| LabeledItem {
                        label: format_task(&task),
                        item: task,
                    })
                    .collect();
                rich_rofi::<LabeledItem<Task>, Task>(
                    "Press enter to go back to choosing an action",
                    labeled_tasks,
                )?;
            }
            _ => {
                let tasks = tw::query("status:pending").unwrap();
                let labeled_tasks: Vec<_> = tasks
                    .into_iter()
                    .map(|task| LabeledItem {
                        label: format_task(&task),
                        item: task,
                    })
                    .collect();
                let mut task: Task = rich_rofi("Choose a task", labeled_tasks)?;
                match action {
                    Action::Done => *task.status_mut() = TaskStatus::Completed,
                    Action::Start => task.set_start(Some(LocalTime::now().naive_local())),
                    Action::Stop => task.set_start::<NaiveDateTime>(None),
                    Action::Delete => *task.status_mut() = TaskStatus::Deleted,
                    Action::Add | Action::List => unreachable!("Already handled this case"),
                }
                tw::save(Some(&task))?;
                break;
            }
        }
    }
    Ok(())
}

enum Action {
    Add,
    Delete,
    Done,
    List,
    Start,
    Stop,
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
            }
        )
    }
}

fn format_task(task: &Task) -> String {
    let id_str = match task.id() {
        Some(n) => n.to_string(),
        None => "-".to_string(),
    };
    format!("[{}] {}", id_str, task.description())
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

fn rich_rofi<T, U>(prompt: &str, items: Vec<T>) -> Result<U, Box<dyn std::error::Error>>
where
    T: Into<LabeledItem<U>>,
{
    let mut items: Vec<LabeledItem<U>> = items.into_iter().map(|i| i.into()).collect();
    let labels = items.iter().map(|i| &i.label).collect();
    let idx = Rofi::new(&labels).prompt(prompt).run_index()?;
    // use `swap_remove` so we don't have to re-order the list we're about the throw away anyways
    Ok(items.swap_remove(idx).item)
}
