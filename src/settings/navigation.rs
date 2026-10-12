use std::collections::BTreeSet;

use oxibar_plugin_api::SettingDescriptor;

use super::Category;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Content {
    Fields,
    Layout,
    Monitors,
    Plugins,
    Desktop,
    Providers,
    Overrides,
}

#[derive(Debug, Clone)]
pub(crate) struct Page {
    pub id: String,
    pub label: String,
    pub description: String,
    pub category: Category,
    pub content: Content,
    pub fields: Vec<String>,
    pub children: Vec<Page>,
}

impl Page {
    fn find(&self, id: &str) -> Option<&Self> {
        if self.id == id {
            Some(self)
        } else {
            self.children.iter().find_map(|child| child.find(id))
        }
    }

    fn ensure(&mut self, segments: &[&str]) -> &mut Self {
        let Some((segment, rest)) = segments.split_first() else {
            return self;
        };
        let id = format!("{}/{segment}", self.id);
        let index = self.children.iter().position(|child| child.id == id);
        let index = index.unwrap_or_else(|| {
            let (label, description, content) = metadata(&id);
            self.children.push(Self {
                id,
                label,
                description,
                category: self.category,
                content,
                fields: Vec::new(),
                children: Vec::new(),
            });
            self.children.len() - 1
        });
        self.children[index].ensure(rest)
    }

    pub fn field_count(&self) -> usize {
        self.fields.len() + self.children.iter().map(Self::field_count).sum::<usize>()
    }
}

pub(crate) struct Navigation {
    pub roots: Vec<Page>,
    pub selected: String,
    pub expanded: BTreeSet<String>,
}

impl Navigation {
    pub fn new(schema: &[SettingDescriptor]) -> Self {
        let mut navigation = Self {
            roots: Category::ALL
                .into_iter()
                .map(|category| Page {
                    id: category.id().into(),
                    label: category.label().into(),
                    description: category.description().into(),
                    category,
                    content: Content::Fields,
                    fields: Vec::new(),
                    children: Vec::new(),
                })
                .collect(),
            selected: Category::Appearance.id().into(),
            expanded: BTreeSet::from([Category::Appearance.id().into()]),
        };
        for id in [
            "appearance/typography",
            "appearance/surfaces",
            "appearance/feedback",
            "bar/layout",
            "bar/geometry",
            "bar/visibility",
            "bar/outputs",
            "bar/menus",
            "launcher/providers",
            "launcher/search",
            "launcher/applications",
            "launcher/overview",
            "plugins/installed",
            "advanced/window",
            "advanced/runtime",
            "advanced/ipc",
            "advanced/overrides",
            "monitors/arrangement",
            "monitors/profiles",
            "monitors/discovery",
            "controls/presentation",
            "controls/widgets",
            "dock/applications",
            "dock/placement",
            "desktop/widgets",
            "desktop/placement",
            "theme/colors",
            "theme/typography",
            "theme/spacing",
            "theme/motion",
            "shortcuts/common",
            "shortcuts/settings",
            "shortcuts/controls",
            "shortcuts/launcher",
            "shortcuts/session",
            "shortcuts/overview",
            "shortcuts/desktop",
        ] {
            navigation.ensure(id);
        }
        for field in schema {
            navigation
                .ensure(&field_page(&field.path))
                .fields
                .push(field.path.clone());
        }
        navigation
    }

    fn ensure(&mut self, id: &str) -> &mut Page {
        let mut segments = id.split('/');
        let root = segments.next().expect("settings page root");
        self.roots
            .iter_mut()
            .find(|page| page.id == root)
            .expect("known settings category")
            .ensure(&segments.collect::<Vec<_>>())
    }

    pub fn find(&self, id: &str) -> Option<&Page> {
        self.roots.iter().find_map(|page| page.find(id))
    }

    pub fn current(&self) -> &Page {
        self.find(&self.selected)
            .expect("selected settings page exists")
    }

    pub fn select(&mut self, id: &str, toggle: bool) -> Option<Category> {
        let page = self.find(id)?;
        let category = page.category;
        let branch = !page.children.is_empty();
        for root in &self.roots {
            if root.category != category {
                self.expanded.remove(&root.id);
            }
        }
        let mut parent = id;
        while let Some((ancestor, _)) = parent.rsplit_once('/') {
            self.expanded.insert(ancestor.into());
            parent = ancestor;
        }
        if branch && (!toggle || !self.expanded.remove(id)) {
            self.expanded.insert(id.into());
        }
        self.selected = id.into();
        Some(category)
    }

    pub fn reconcile(&mut self, schema: &[SettingDescriptor]) {
        let mut next = Self::new(schema);
        next.expanded = self
            .expanded
            .iter()
            .filter(|id| next.find(id).is_some())
            .cloned()
            .collect();
        let mut selected = self.selected.as_str();
        while next.find(selected).is_none() {
            selected = selected
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or(Category::Plugins.id());
        }
        next.selected = selected.into();
        *self = next;
    }

    pub fn breadcrumbs(&self) -> Vec<&Page> {
        self.selected
            .match_indices('/')
            .map(|(index, _)| &self.selected[..index])
            .chain(std::iter::once(self.selected.as_str()))
            .filter_map(|id| self.find(id))
            .collect()
    }

    pub fn choices(&self) -> Vec<Choice> {
        fn append(page: &Page, prefix: &str, choices: &mut Vec<Choice>) {
            let label = if prefix.is_empty() {
                page.label.clone()
            } else {
                format!("{prefix} › {}", page.label)
            };
            choices.push(Choice {
                id: page.id.clone(),
                label: label.clone(),
            });
            for child in &page.children {
                append(child, &label, choices);
            }
        }
        let mut choices = Vec::new();
        for page in &self.roots {
            append(page, "", &mut choices);
        }
        choices
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub id: String,
    pub label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

fn field_page(path: &str) -> String {
    let category = Category::ALL
        .into_iter()
        .find(|category| category.accepts(path))
        .unwrap_or(Category::Plugins);
    let section = match category {
        Category::Appearance if path.starts_with("toasts.") => "feedback/notifications",
        Category::Appearance if path.starts_with("osd.") => "feedback/indicators",
        Category::Appearance if path.starts_with("bar.font") => "typography",
        Category::Appearance => "surfaces",
        Category::Bar if ["bar.start", "bar.center", "bar.end"].contains(&path) => "layout",
        Category::Bar if path.starts_with("menus.session_") => "menus/session",
        Category::Bar if path.starts_with("menus.") => "menus/inbox",
        Category::Bar if ["bar.height", "bar.width", "bar.position"].contains(&path) => "geometry",
        Category::Bar if path.starts_with("bar.output") || path == "bar.profiles" => "outputs",
        Category::Bar => "visibility",
        Category::Launcher if path == "launcher.default_plugins" => "providers",
        Category::Launcher if path.starts_with("applications.") => "applications",
        Category::Launcher if path.starts_with("overview.") => "overview",
        Category::Launcher => "search",
        Category::Advanced if path.starts_with("settings.") => "window",
        Category::Advanced if path.starts_with("ipc.") => "ipc",
        Category::Advanced => "runtime",
        Category::Monitors if path == "monitor.refresh_seconds" => "discovery",
        Category::Monitors => "profiles",
        Category::ControlCenter if path == "control_center.widgets" => "widgets",
        Category::ControlCenter => "presentation",
        Category::Dock if path == "dock.pinned" || path == "dock.show_running" => "applications",
        Category::Dock => "placement",
        Category::Desktop if path == "desktop.widgets" => "widgets",
        Category::Desktop => "placement",
        Category::Theme if path.starts_with("theme.base24.") => "colors/base24",
        Category::Theme if path.starts_with("theme.palette.") => "colors/roles",
        Category::Theme if path == "theme.accent" => "colors/accent",
        Category::Theme if path.starts_with("theme.font") => "typography",
        Category::Theme if path == "theme.animation_ms" || path == "theme.reduced_motion" => {
            "motion"
        }
        Category::Theme => "spacing",
        Category::Shortcuts | Category::Plugins => {
            let prefix = path.split('.').next().unwrap_or(path);
            if category == Category::Plugins {
                return if path == "plugins" {
                    "plugins/installed".into()
                } else {
                    let mut route = format!("plugins/{prefix}");
                    for segment in path
                        .split('.')
                        .skip(1)
                        .take(path.split('.').count().saturating_sub(2))
                    {
                        route.push('/');
                        route.push_str(segment);
                    }
                    route
                };
            }
            return format!(
                "shortcuts/{}",
                path.split('.')
                    .nth(1)
                    .filter(|_| path.matches('.').count() > 1)
                    .unwrap_or("common")
            );
        }
    };
    format!("{}/{section}", category.id())
}

fn metadata(id: &str) -> (String, String, Content) {
    let (label, description, content) = match id {
        "bar/layout" => (
            "Widget arrangement",
            "Drag widgets between the three bar sections.",
            Content::Layout,
        ),
        "monitors/arrangement" => (
            "Arrange displays",
            "Drag to snap display edges, then test and confirm the layout.",
            Content::Monitors,
        ),
        "plugins/installed" => (
            "Installed plugins",
            "Enable providers without restarting the shell.",
            Content::Plugins,
        ),
        "desktop/widgets" => (
            "Widget instances",
            "Manage independent widgets and enter desktop edit mode.",
            Content::Desktop,
        ),
        "launcher/providers" => (
            "Search providers",
            "Choose the applications and tools available in the launcher.",
            Content::Providers,
        ),
        "advanced/overrides" => (
            "GUI overrides",
            "Edit sparse TOML while the base / Nix configuration stays read-only.",
            Content::Overrides,
        ),
        "appearance/feedback" => (
            "Notifications & feedback",
            "Place notifications, music and device indicators.",
            Content::Fields,
        ),
        "appearance/feedback/notifications" => (
            "Notification toasts",
            "Shared placement for notifications and music.",
            Content::Fields,
        ),
        "appearance/feedback/indicators" => (
            "Device indicators",
            "Volume, brightness and other on-screen feedback.",
            Content::Fields,
        ),
        "theme/colors/base24" => (
            "Base24 palette",
            "Override individual scheme colors.",
            Content::Fields,
        ),
        "theme/colors/roles" => (
            "Semantic colors",
            "Fine-tune colors by their UI role.",
            Content::Fields,
        ),
        "bar/menus/inbox" => (
            "Notification inbox",
            "Choose the inbox presentation and dimensions.",
            Content::Fields,
        ),
        "bar/menus/session" => (
            "Session menu",
            "Choose the actions shown in the dedicated session tiles.",
            Content::Fields,
        ),
        "advanced/window" => (
            "Settings window",
            "Choose the settings surface dimensions.",
            Content::Fields,
        ),
        "advanced/runtime" => (
            "Shell runtime",
            "Startup recovery and instance ownership.",
            Content::Fields,
        ),
        "monitors/profiles" => (
            "Saved layouts",
            "Control startup restoration and saved connector profiles.",
            Content::Fields,
        ),
        "monitors/discovery" => (
            "Display discovery",
            "Tune display inventory refreshes.",
            Content::Fields,
        ),
        _ => {
            return (
                title(id.rsplit('/').next().unwrap_or(id)),
                "Tune this group of preferences.".into(),
                Content::Fields,
            );
        }
    };
    (label.into(), description.into(), content)
}

fn title(value: &str) -> String {
    value
        .split('_')
        .map(|word| {
            let mut letters = word.chars();
            letters
                .next()
                .map(|first| format!("{}{rest}", first.to_uppercase(), rest = letters.as_str()))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_descriptor_has_one_leaf_and_nested_routes_are_stable() {
        let mut schema = crate::config::schema::host_schema();
        schema.push(SettingDescriptor::new(
            "weather.location.units",
            "Units",
            oxibar_plugin_api::SettingKind::Text,
            None,
            oxibar_plugin_api::ApplyMode::Live,
        ));
        let navigation = Navigation::new(&schema);
        for field in &schema {
            let copies = navigation
                .choices()
                .iter()
                .filter_map(|choice| navigation.find(&choice.id))
                .flat_map(|page| &page.fields)
                .filter(|path| *path == &field.path)
                .count();
            assert_eq!(copies, 1, "{}", field.path);
        }
        assert!(
            navigation
                .find("appearance/feedback/notifications")
                .is_some()
        );
        assert!(navigation.find("theme/colors/base24").is_some());
        assert!(navigation.find("plugins/weather/location").is_some());
    }

    #[test]
    fn expansion_selection_and_schema_reconciliation_preserve_navigation() {
        let mut schema = crate::config::schema::host_schema();
        let mut navigation = Navigation::new(&schema);
        navigation
            .select("appearance/feedback/notifications", false)
            .unwrap();
        assert!(navigation.expanded.contains("appearance"));
        assert!(navigation.expanded.contains("appearance/feedback"));
        navigation.select("appearance/feedback", true).unwrap();
        assert!(!navigation.expanded.contains("appearance/feedback"));
        assert_eq!(navigation.selected, "appearance/feedback");
        assert!(navigation.select("unknown/page", false).is_none());
        schema.push(SettingDescriptor::new(
            "weather.location.units",
            "Units",
            oxibar_plugin_api::SettingKind::Text,
            None,
            oxibar_plugin_api::ApplyMode::Live,
        ));
        navigation.reconcile(&schema);
        navigation
            .select("plugins/weather/location", false)
            .unwrap();
        schema.pop();
        navigation.reconcile(&schema);
        assert_eq!(navigation.selected, "plugins");
        assert_eq!(navigation.breadcrumbs().len(), 1);
    }
}
