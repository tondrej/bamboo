//! The [`ThemeEngine`] that renders a [`Site`] to disk
//! using Tera templates from the built-in default theme (embedded via
//! `include_str!`) or a user-supplied theme directory.

use crate::assets::AssetConfig;
use crate::error::Result;
use crate::feeds;
use crate::images;
use crate::redirects;
use crate::search;
use crate::sitemap;
use crate::theme_filters::register_custom_filters;
use crate::theme_render::{
    copy_assets, render_404, render_collection, render_index, render_page, render_pagination,
    render_post, render_search, site_metadata,
};
use crate::types::Site;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tera::Tera;
use walkdir::WalkDir;

const DEFAULT_BASE_TEMPLATE: &str = include_str!("../themes/default/templates/base.html");
const DEFAULT_INDEX_TEMPLATE: &str = include_str!("../themes/default/templates/index.html");
const DEFAULT_PAGE_TEMPLATE: &str = include_str!("../themes/default/templates/page.html");
const DEFAULT_POST_TEMPLATE: &str = include_str!("../themes/default/templates/post.html");
const DEFAULT_COLLECTION_TEMPLATE: &str =
    include_str!("../themes/default/templates/collection.html");
const DEFAULT_COLLECTION_ITEM_TEMPLATE: &str =
    include_str!("../themes/default/templates/collection_item.html");
const DEFAULT_SLIDESHOW_TEMPLATE: &str = include_str!("../themes/default/templates/slideshow.html");
const DEFAULT_DOCS_TEMPLATE: &str = include_str!("../themes/default/templates/docs.html");
const DEFAULT_PORTFOLIO_TEMPLATE: &str = include_str!("../themes/default/templates/portfolio.html");
const DEFAULT_LANDING_TEMPLATE: &str = include_str!("../themes/default/templates/landing.html");
const DEFAULT_CHANGELOG_TEMPLATE: &str = include_str!("../themes/default/templates/changelog.html");
const DEFAULT_BOOK_TEMPLATE: &str = include_str!("../themes/default/templates/book.html");
const DEFAULT_ARCHIVE_TEMPLATE: &str = include_str!("../themes/default/templates/archive.html");
const DEFAULT_CATEGORIES_GROUPED_TEMPLATE: &str =
    include_str!("../themes/default/templates/categories_grouped.html");
const DEFAULT_TAGS_GROUPED_TEMPLATE: &str =
    include_str!("../themes/default/templates/tags_grouped.html");
const DEFAULT_TAGS_TEMPLATE: &str = include_str!("../themes/default/templates/tags.html");
const DEFAULT_TAG_TEMPLATE: &str = include_str!("../themes/default/templates/tag.html");
const DEFAULT_CATEGORIES_TEMPLATE: &str =
    include_str!("../themes/default/templates/categories.html");
const DEFAULT_CATEGORY_TEMPLATE: &str = include_str!("../themes/default/templates/category.html");
const DEFAULT_TAXONOMY_TEMPLATE: &str = include_str!("../themes/default/templates/taxonomy.html");
const DEFAULT_TAXONOMY_TERM_TEMPLATE: &str =
    include_str!("../themes/default/templates/taxonomy_term.html");
const DEFAULT_PAGINATION_TEMPLATE: &str =
    include_str!("../themes/default/templates/pagination.html");
const DEFAULT_404_TEMPLATE: &str = include_str!("../themes/default/templates/404.html");
const DEFAULT_HEADER_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/header.html");
const DEFAULT_FOOTER_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/footer.html");
const DEFAULT_NAV_PARTIAL: &str = include_str!("../themes/default/templates/partials/nav.html");
const DEFAULT_AUTHOR_PROFILE_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/author_profile.html");
const DEFAULT_POST_BREADCRUMBS_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/post_breadcrumbs.html");
const DEFAULT_POST_HEADER_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/post_header.html");
const DEFAULT_POST_HERO_IMAGE_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/post_hero_image.html");
const DEFAULT_POST_TOC_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/post_toc.html");
const DEFAULT_POST_SHARE_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/post_share.html");
const DEFAULT_POST_RELATED_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/post_related.html");
const DEFAULT_POST_PREV_NEXT_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/post_prev_next.html");
const DEFAULT_POST_EDIT_LINK_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/post_edit_link.html");
const DEFAULT_PAGE_EDIT_LINK_PARTIAL: &str =
    include_str!("../themes/default/templates/partials/page_edit_link.html");
const DEFAULT_SEARCH_TEMPLATE: &str = include_str!("../themes/default/templates/search.html");
const DEFAULT_STYLESHEET: &str = include_str!("../themes/default/static/style.css");

/// Renders a loaded [`Site`] to disk using Tera templates from a theme.
///
/// # Example
///
/// ```no_run
/// use bamboo_ssg::{SiteBuilder, ThemeEngine};
/// let site = SiteBuilder::new("./my-site").build()?;
/// let engine = ThemeEngine::new("default")?;
/// engine.render_site(&site, std::path::Path::new("./dist"))?;
/// # Ok::<_, bamboo_ssg::BambooError>(())
/// ```
pub struct ThemeEngine {
    tera: Tera,
    theme_static_dir: Option<PathBuf>,
    override_static_dir: Option<PathBuf>,
    is_builtin_default: bool,
}

impl ThemeEngine {
    /// Loads a theme by name. If `theme` is a directory path that exists on
    /// disk, it is used directly; the literal value `"default"` loads the
    /// built-in theme that ships compiled into the crate.
    pub fn new(theme: &str) -> Result<Self> {
        let theme_path = Path::new(theme);

        if theme_path.exists() && theme_path.is_dir() {
            Self::from_directory(theme_path)
        } else if theme == "default" {
            Self::builtin_default()
        } else {
            Err(crate::error::BambooError::ThemeNotFound {
                name: theme.to_string(),
            })
        }
    }

    /// Like [`ThemeEngine::new`] but also loads site-level template and
    /// static-file overrides from `override_dir`. Templates in the override
    /// directory shadow theme templates of the same name.
    pub fn new_with_overrides(theme: &str, override_dir: &Path) -> Result<Self> {
        let mut engine = Self::new(theme)?;
        engine.apply_overrides(override_dir)?;
        Ok(engine)
    }

    fn apply_overrides(&mut self, override_dir: &Path) -> Result<()> {
        let templates_dir = override_dir.join("templates");
        if !templates_dir.exists() {
            return Ok(());
        }

        for entry in WalkDir::new(&templates_dir)
            .min_depth(1)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path
                .extension()
                .map(|extension| extension != "html")
                .unwrap_or(true)
            {
                continue;
            }
            let relative = path.strip_prefix(&templates_dir).map_err(|_| {
                crate::error::BambooError::InvalidPath {
                    path: path.to_path_buf(),
                }
            })?;
            let template_name = relative.to_string_lossy().replace('\\', "/");
            let content = fs::read_to_string(path)?;
            self.tera.add_raw_template(&template_name, &content)?;
        }

        let static_dir = override_dir.join("static");
        if static_dir.exists() {
            self.override_static_dir = Some(static_dir);
        }

        Ok(())
    }

    fn builtin_default() -> Result<Self> {
        let mut tera = Tera::default();

        tera.add_raw_template("base.html", DEFAULT_BASE_TEMPLATE)?;
        tera.add_raw_template("index.html", DEFAULT_INDEX_TEMPLATE)?;
        tera.add_raw_template("page.html", DEFAULT_PAGE_TEMPLATE)?;
        tera.add_raw_template("post.html", DEFAULT_POST_TEMPLATE)?;
        tera.add_raw_template("collection.html", DEFAULT_COLLECTION_TEMPLATE)?;
        tera.add_raw_template("collection_item.html", DEFAULT_COLLECTION_ITEM_TEMPLATE)?;
        tera.add_raw_template("slideshow.html", DEFAULT_SLIDESHOW_TEMPLATE)?;
        tera.add_raw_template("docs.html", DEFAULT_DOCS_TEMPLATE)?;
        tera.add_raw_template("portfolio.html", DEFAULT_PORTFOLIO_TEMPLATE)?;
        tera.add_raw_template("landing.html", DEFAULT_LANDING_TEMPLATE)?;
        tera.add_raw_template("changelog.html", DEFAULT_CHANGELOG_TEMPLATE)?;
        tera.add_raw_template("book.html", DEFAULT_BOOK_TEMPLATE)?;
        tera.add_raw_template("archive.html", DEFAULT_ARCHIVE_TEMPLATE)?;
        tera.add_raw_template(
            "categories_grouped.html",
            DEFAULT_CATEGORIES_GROUPED_TEMPLATE,
        )?;
        tera.add_raw_template("tags_grouped.html", DEFAULT_TAGS_GROUPED_TEMPLATE)?;
        tera.add_raw_template("tags.html", DEFAULT_TAGS_TEMPLATE)?;
        tera.add_raw_template("tag.html", DEFAULT_TAG_TEMPLATE)?;
        tera.add_raw_template("categories.html", DEFAULT_CATEGORIES_TEMPLATE)?;
        tera.add_raw_template("category.html", DEFAULT_CATEGORY_TEMPLATE)?;
        tera.add_raw_template("taxonomy.html", DEFAULT_TAXONOMY_TEMPLATE)?;
        tera.add_raw_template("taxonomy_term.html", DEFAULT_TAXONOMY_TERM_TEMPLATE)?;
        tera.add_raw_template("pagination.html", DEFAULT_PAGINATION_TEMPLATE)?;
        tera.add_raw_template("404.html", DEFAULT_404_TEMPLATE)?;
        tera.add_raw_template("partials/header.html", DEFAULT_HEADER_PARTIAL)?;
        tera.add_raw_template("partials/footer.html", DEFAULT_FOOTER_PARTIAL)?;
        tera.add_raw_template("partials/nav.html", DEFAULT_NAV_PARTIAL)?;
        tera.add_raw_template(
            "partials/author_profile.html",
            DEFAULT_AUTHOR_PROFILE_PARTIAL,
        )?;
        tera.add_raw_template(
            "partials/post_breadcrumbs.html",
            DEFAULT_POST_BREADCRUMBS_PARTIAL,
        )?;
        tera.add_raw_template("partials/post_header.html", DEFAULT_POST_HEADER_PARTIAL)?;
        tera.add_raw_template(
            "partials/post_hero_image.html",
            DEFAULT_POST_HERO_IMAGE_PARTIAL,
        )?;
        tera.add_raw_template("partials/post_toc.html", DEFAULT_POST_TOC_PARTIAL)?;
        tera.add_raw_template("partials/post_share.html", DEFAULT_POST_SHARE_PARTIAL)?;
        tera.add_raw_template("partials/post_related.html", DEFAULT_POST_RELATED_PARTIAL)?;
        tera.add_raw_template(
            "partials/post_prev_next.html",
            DEFAULT_POST_PREV_NEXT_PARTIAL,
        )?;
        tera.add_raw_template(
            "partials/post_edit_link.html",
            DEFAULT_POST_EDIT_LINK_PARTIAL,
        )?;
        tera.add_raw_template(
            "partials/page_edit_link.html",
            DEFAULT_PAGE_EDIT_LINK_PARTIAL,
        )?;
        tera.add_raw_template("search.html", DEFAULT_SEARCH_TEMPLATE)?;

        register_custom_filters(&mut tera);

        Ok(Self {
            tera,
            theme_static_dir: None,
            override_static_dir: None,
            is_builtin_default: true,
        })
    }

    fn from_directory(theme_dir: &Path) -> Result<Self> {
        let templates_dir = theme_dir.join("templates");
        let static_dir = theme_dir.join("static");

        let escaped_templates =
            escape_glob_path(&templates_dir.to_string_lossy().replace('\\', "/"));
        let pattern_str = format!("{escaped_templates}/**/*.html");

        let mut tera = Tera::new(&pattern_str)?;
        register_custom_filters(&mut tera);

        let theme_static_dir = if static_dir.exists() {
            Some(static_dir)
        } else {
            None
        };

        Ok(Self {
            tera,
            theme_static_dir,
            override_static_dir: None,
            is_builtin_default: false,
        })
    }

    /// Renders every page, post, collection item, taxonomy page, feed, and
    /// sitemap into `output_dir`. Performs a full build.
    pub fn render_site(&self, site: &Site, output_dir: &Path) -> Result<()> {
        self.render_site_with_targets(site, output_dir, None)
    }

    /// Incremental variant of [`ThemeEngine::render_site`]. Only writes
    /// output for the [`RenderTarget`](crate::cache::RenderTarget)s listed in
    /// `targets`; use [`classify_changes`](crate::cache::classify_changes) +
    /// [`expand_targets`](crate::cache::expand_targets) to compute the set.
    pub fn render_site_with_targets(
        &self,
        site: &Site,
        output_dir: &Path,
        targets: Option<&std::collections::HashSet<crate::cache::RenderTarget>>,
    ) -> Result<()> {
        use crate::cache::{
            RenderTarget, should_render, should_render_any_collection, should_render_any_page,
            should_render_any_post,
        };

        let render_all =
            targets.is_none() || targets.is_some_and(|t| t.contains(&RenderTarget::All));

        fs::create_dir_all(output_dir)?;

        if render_all && self.is_builtin_default {
            fs::write(output_dir.join("style.css"), DEFAULT_STYLESHEET)?;
        }

        if render_all
            || targets.is_some_and(|t| should_render(t, &RenderTarget::Page("index".to_string())))
        {
            render_index(&self.tera, site, output_dir)?;
        }

        if render_all {
            site.pages
                .par_iter()
                .filter(|page| page.content.slug != "404")
                .try_for_each(|page| render_page(&self.tera, site, page, output_dir))?;
        } else if let Some(target_set) = targets
            && should_render_any_page(target_set)
        {
            site.pages
                .par_iter()
                .filter(|page| {
                    page.content.slug != "404"
                        && should_render(target_set, &RenderTarget::Page(page.content.slug.clone()))
                })
                .try_for_each(|page| render_page(&self.tera, site, page, output_dir))?;
        }

        let post_tuples: Vec<_> = site
            .posts
            .iter()
            .enumerate()
            .map(|(index, post)| {
                let prev_post = if index + 1 < site.posts.len() {
                    Some(&site.posts[index + 1])
                } else {
                    None
                };
                let next_post = if index > 0 {
                    Some(&site.posts[index - 1])
                } else {
                    None
                };
                (post, prev_post, next_post)
            })
            .collect();

        if render_all {
            post_tuples
                .par_iter()
                .try_for_each(|(post, prev_post, next_post)| {
                    render_post(&self.tera, site, post, *prev_post, *next_post, output_dir)
                })?;
        } else if let Some(target_set) = targets
            && should_render_any_post(target_set)
        {
            post_tuples
                .par_iter()
                .filter(|(post, _, _)| {
                    should_render(target_set, &RenderTarget::Post(post.content.slug.clone()))
                })
                .try_for_each(|(post, prev_post, next_post)| {
                    render_post(&self.tera, site, post, *prev_post, *next_post, output_dir)
                })?;
        }

        if render_all {
            site.collections
                .par_iter()
                .try_for_each(|(name, collection)| {
                    render_collection(&self.tera, site, name, collection, output_dir)
                })?;
        } else if let Some(target_set) = targets
            && should_render_any_collection(target_set)
        {
            site.collections
                .par_iter()
                .filter(|(name, _)| {
                    should_render(target_set, &RenderTarget::Collection(name.to_string()))
                })
                .try_for_each(|(name, collection)| {
                    render_collection(&self.tera, site, name, collection, output_dir)
                })?;
        }

        if render_all || targets.is_some_and(|t| should_render(t, &RenderTarget::Pagination)) {
            render_pagination(&self.tera, site, output_dir)?;
        }

        if render_all || targets.is_some_and(|t| t.contains(&RenderTarget::AllTaxonomies)) {
            let metadata = site_metadata(site);
            crate::taxonomy::render_all_taxonomies(&self.tera, site, &metadata, output_dir)?;
        }

        if render_all {
            render_404(&self.tera, site, output_dir)?;
        }

        if render_all || targets.is_some_and(|t| should_render(t, &RenderTarget::SearchIndex)) {
            render_search(&self.tera, site, output_dir)?;
        }

        if render_all {
            self.copy_theme_static(output_dir)?;
            copy_assets(&site.assets, output_dir)?;
        }

        if render_all || targets.is_some_and(|t| should_render(t, &RenderTarget::Feeds)) {
            feeds::generate_rss(site, output_dir)?;
            feeds::generate_atom(site, output_dir)?;
            for (name, collection) in &site.collections {
                feeds::generate_collection_rss(site, name, collection, output_dir)?;
                feeds::generate_collection_atom(site, name, collection, output_dir)?;
            }
        }

        if render_all || targets.is_some_and(|t| should_render(t, &RenderTarget::Sitemap)) {
            sitemap::generate_sitemap(site, output_dir)?;
        }

        if render_all {
            redirects::generate_redirects(site, output_dir)?;
        }

        if render_all || targets.is_some_and(|t| should_render(t, &RenderTarget::SearchIndex)) {
            search::generate_search_index(site, output_dir)?;
        }

        if let Some(ref image_config) = site.config.images
            && render_all
        {
            let manifest = images::process_images(output_dir, image_config)?;
            images::apply_srcset_to_html(output_dir, &manifest)?;
        }

        if render_all {
            let mut sass_load_paths = Vec::new();
            if let Some(ref static_dir) = self.theme_static_dir {
                sass_load_paths.push(static_dir.clone());
            }
            if let Some(ref override_dir) = self.override_static_dir {
                sass_load_paths.push(override_dir.clone());
            }

            let asset_config = AssetConfig {
                minify: site.config.minify,
                fingerprint: site.config.fingerprint,
                base_url: site.config.base_url.clone(),
                sass_load_paths,
            };
            crate::assets::process_assets(output_dir, &asset_config)?;
        }

        Ok(())
    }

    fn copy_theme_static(&self, output_dir: &Path) -> Result<()> {
        self.copy_static_dir(&self.theme_static_dir, output_dir)?;
        self.copy_static_dir(&self.override_static_dir, output_dir)?;
        Ok(())
    }

    fn copy_static_dir(&self, static_dir: &Option<PathBuf>, output_dir: &Path) -> Result<()> {
        if let Some(static_dir) = static_dir {
            for entry in WalkDir::new(static_dir)
                .min_depth(1)
                .into_iter()
                .filter_map(|entry| entry.ok())
            {
                let path = entry.path();

                if !path.is_file() {
                    continue;
                }

                let relative = path.strip_prefix(static_dir).unwrap();
                let dest = output_dir.join(relative);

                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }

                fs::copy(path, &dest)?;
            }
        }

        Ok(())
    }
}

fn escape_glob_path(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for character in path.chars() {
        match character {
            '[' | ']' | '{' | '}' | '*' | '?' => {
                escaped.push('[');
                escaped.push(character);
                escaped.push(']');
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

fn is_filesystem_root(path: &Path) -> bool {
    if path.parent().is_none() {
        return true;
    }
    if path.parent() == Some(Path::new("")) {
        return true;
    }
    let path_str = path.to_string_lossy();
    let stripped = path_str.trim_start_matches("\\\\?\\");
    if stripped.len() <= 3 && stripped.ends_with('\\') && stripped.chars().nth(1) == Some(':') {
        return true;
    }
    if stripped.starts_with("\\\\") || stripped.starts_with("UNC\\") {
        let without_prefix = if let Some(rest) = stripped.strip_prefix("UNC\\") {
            rest
        } else if let Some(rest) = stripped.strip_prefix("\\\\") {
            rest
        } else {
            stripped
        };
        let slash_count = without_prefix.matches('\\').count();
        if slash_count <= 1 {
            return true;
        }
    }
    false
}

fn is_direct_child_of_root(path: &Path) -> bool {
    if let Some(parent) = path.parent() {
        is_filesystem_root(parent)
    } else {
        true
    }
}

/// Removes every file and subdirectory under `output_dir`, preserving the
/// directory itself. Safe to call on a non-existent path.
pub fn clean_output_dir(output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        let canonical =
            output_dir
                .canonicalize()
                .map_err(|_| crate::error::BambooError::InvalidPath {
                    path: output_dir.to_path_buf(),
                })?;
        if is_filesystem_root(&canonical) {
            return Err(crate::error::BambooError::InvalidPath {
                path: output_dir.to_path_buf(),
            });
        }
        if is_direct_child_of_root(&canonical) {
            return Err(crate::error::BambooError::InvalidPath {
                path: output_dir.to_path_buf(),
            });
        }
        if let Some(home) = dirs_home()
            && canonical == home.canonicalize().unwrap_or(home)
        {
            return Err(crate::error::BambooError::InvalidPath {
                path: output_dir.to_path_buf(),
            });
        }
        if canonical.join("bamboo.toml").exists() {
            return Err(crate::error::BambooError::InvalidPath {
                path: output_dir.to_path_buf(),
            });
        }
        if let Ok(current_dir) = std::env::current_dir()
            && let Ok(canonical_current) = current_dir.canonicalize()
            && canonical == canonical_current
        {
            return Err(crate::error::BambooError::InvalidPath {
                path: output_dir.to_path_buf(),
            });
        }
        fs::remove_dir_all(output_dir)?;
    }
    Ok(())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_glob_path_no_special() {
        assert_eq!(
            escape_glob_path("/home/user/templates"),
            "/home/user/templates"
        );
    }

    #[test]
    fn test_escape_glob_path_with_brackets() {
        assert_eq!(escape_glob_path("path/[test]"), "path/[[]test[]]");
    }

    #[test]
    fn test_escape_glob_path_with_braces() {
        assert_eq!(escape_glob_path("path/{test}"), "path/[{]test[}]");
    }

    #[test]
    fn test_escape_glob_path_with_wildcards() {
        assert_eq!(escape_glob_path("path/*.html"), "path/[*].html");
    }

    #[test]
    fn test_is_filesystem_root_unix_root() {
        assert!(is_filesystem_root(Path::new("/")));
    }

    #[test]
    fn test_is_filesystem_root_normal_path() {
        assert!(!is_filesystem_root(Path::new("/home/user/project")));
    }

    #[test]
    fn test_is_direct_child_of_root() {
        assert!(!is_direct_child_of_root(Path::new("/home/user/project")));
    }

    #[test]
    fn test_clean_output_dir_nonexistent() {
        let result = clean_output_dir(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_clean_output_dir_removes_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let output = dir.path().join("output");
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("test.html"), "test").unwrap();

        clean_output_dir(&output).unwrap();
        assert!(!output.exists());
    }

    #[test]
    fn test_clean_output_dir_rejects_project_root() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("bamboo.toml"), "title = \"Test\"").unwrap();

        let result = clean_output_dir(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_builtin_default_theme() {
        let engine = ThemeEngine::new("default").unwrap();
        assert!(engine.is_builtin_default);
    }

    #[test]
    fn test_nonexistent_theme_error() {
        let result = ThemeEngine::new("nonexistent-theme-12345");
        assert!(result.is_err());
    }

    #[test]
    fn test_render_site_basic() {
        use crate::types::*;
        use std::collections::HashMap;

        let site = Site {
            config: SiteConfig {
                title: "Test".to_string(),
                base_url: "https://example.com".to_string(),
                description: None,
                author: None,
                language: None,
                posts_per_page: 10,
                minify: false,
                fingerprint: false,
                images: None,
                syntax_theme: crate::types::default_syntax_theme(),
                taxonomies: crate::types::default_taxonomies(),
                math: false,
                favicon: None,
                link_check_ignore: Vec::new(),
                extra: HashMap::new(),
            },
            home: None,
            pages: vec![],
            posts: vec![],
            collections: HashMap::new(),
            data: HashMap::new(),
            assets: vec![],
        };

        let output_dir = tempfile::TempDir::new().unwrap();
        let engine = ThemeEngine::new("default").unwrap();
        engine.render_site(&site, output_dir.path()).unwrap();

        assert!(output_dir.path().join("index.html").exists());
        assert!(output_dir.path().join("404.html").exists());
        assert!(output_dir.path().join("style.css").exists());
        assert!(output_dir.path().join("rss.xml").exists());
        assert!(output_dir.path().join("atom.xml").exists());
        assert!(output_dir.path().join("sitemap.xml").exists());
        assert!(output_dir.path().join("search-index.json").exists());
    }

    #[test]
    fn test_render_site_with_posts() {
        use crate::types::*;
        use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
        use std::collections::HashMap;

        let date = Utc.from_utc_datetime(
            &NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_time(NaiveTime::MIN),
        );

        let site = Site {
            config: SiteConfig {
                title: "Test Blog".to_string(),
                base_url: "https://example.com".to_string(),
                description: Some("A test blog".to_string()),
                author: Some("Author".to_string()),
                language: Some("en".to_string()),
                posts_per_page: 10,
                minify: false,
                fingerprint: false,
                images: None,
                syntax_theme: crate::types::default_syntax_theme(),
                taxonomies: crate::types::default_taxonomies(),
                math: false,
                favicon: None,
                link_check_ignore: Vec::new(),
                extra: HashMap::new(),
            },
            home: None,
            pages: vec![Page {
                content: Content {
                    slug: "about".to_string(),
                    title: "About".to_string(),
                    html: "<p>About page</p>".to_string(),
                    frontmatter: Frontmatter::default(),
                    path: PathBuf::from("about/index.html"),
                    template: None,
                    weight: 0,
                    word_count: 2,
                    reading_time: 1,
                    toc: vec![],
                    url: "/about/".to_string(),
                    excerpt: None,
                },
                draft: false,
                redirect_from: vec![],
            }],
            posts: vec![Post {
                content: Content {
                    slug: "hello".to_string(),
                    title: "Hello".to_string(),
                    html: "<p>Hello world</p>".to_string(),
                    frontmatter: Frontmatter::default(),
                    path: PathBuf::from("posts/hello/index.html"),
                    template: None,
                    weight: 0,
                    word_count: 2,
                    reading_time: 1,
                    toc: vec![],
                    url: "/posts/hello/".to_string(),
                    excerpt: Some("Hello world".to_string()),
                },
                date,
                draft: false,
                tags: vec!["test".to_string()],
                categories: vec!["general".to_string()],
                taxonomies_map: HashMap::from([
                    ("tags".to_string(), vec!["test".to_string()]),
                    ("categories".to_string(), vec!["general".to_string()]),
                ]),
                redirect_from: vec![],
            }],
            collections: HashMap::new(),
            data: HashMap::new(),
            assets: vec![],
        };

        let output_dir = tempfile::TempDir::new().unwrap();
        let engine = ThemeEngine::new("default").unwrap();
        engine.render_site(&site, output_dir.path()).unwrap();

        assert!(output_dir.path().join("about/index.html").exists());
        assert!(output_dir.path().join("posts/hello/index.html").exists());
        assert!(output_dir.path().join("tags/index.html").exists());
        assert!(output_dir.path().join("tags/test/index.html").exists());
        assert!(output_dir.path().join("categories/index.html").exists());
        assert!(
            output_dir
                .path()
                .join("categories/general/index.html")
                .exists()
        );
        assert!(output_dir.path().join("search/index.html").exists());
    }

    #[test]
    fn test_render_pagination() {
        use crate::types::*;
        use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
        use std::collections::HashMap;

        let date = Utc.from_utc_datetime(
            &NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_time(NaiveTime::MIN),
        );

        let mut posts = Vec::new();
        for index in 0..3 {
            posts.push(Post {
                content: Content {
                    slug: format!("post-{}", index),
                    title: format!("Post {}", index),
                    html: format!("<p>Post {}</p>", index),
                    frontmatter: Frontmatter::default(),
                    path: PathBuf::from(format!("posts/post-{}/index.html", index)),
                    template: None,
                    weight: 0,
                    word_count: 2,
                    reading_time: 1,
                    toc: vec![],
                    url: format!("/posts/post-{}/", index),
                    excerpt: None,
                },
                date,
                draft: false,
                tags: vec![],
                categories: vec![],
                taxonomies_map: HashMap::new(),
                redirect_from: vec![],
            });
        }

        let site = Site {
            config: SiteConfig {
                title: "Test".to_string(),
                base_url: "https://example.com".to_string(),
                description: None,
                author: None,
                language: None,
                posts_per_page: 1,
                minify: false,
                fingerprint: false,
                images: None,
                syntax_theme: crate::types::default_syntax_theme(),
                taxonomies: crate::types::default_taxonomies(),
                math: false,
                favicon: None,
                link_check_ignore: Vec::new(),
                extra: HashMap::new(),
            },
            home: None,
            pages: vec![],
            posts,
            collections: HashMap::new(),
            data: HashMap::new(),
            assets: vec![],
        };

        let output_dir = tempfile::TempDir::new().unwrap();
        let engine = ThemeEngine::new("default").unwrap();
        engine.render_site(&site, output_dir.path()).unwrap();

        assert!(output_dir.path().join("page/2/index.html").exists());
        assert!(output_dir.path().join("page/3/index.html").exists());
    }

    #[test]
    fn test_render_collection_pagination() {
        use crate::types::*;
        use std::collections::HashMap;

        let items: Vec<CollectionItem> = (0..3)
            .map(|index| CollectionItem {
                content: Content {
                    slug: format!("item-{}", index),
                    title: format!("Item {}", index),
                    html: format!("<p>Item {}</p>", index),
                    frontmatter: Frontmatter::default(),
                    path: PathBuf::from(format!("docs/item-{}/index.html", index)),
                    template: None,
                    weight: 0,
                    word_count: 2,
                    reading_time: 1,
                    toc: vec![],
                    url: format!("/docs/item-{}/", index),
                    excerpt: None,
                },
            })
            .collect();

        let mut collections = HashMap::new();
        collections.insert(
            "docs".to_string(),
            Collection {
                name: "docs".to_string(),
                items,
            },
        );

        let site = Site {
            config: SiteConfig {
                title: "Test".to_string(),
                base_url: "https://example.com".to_string(),
                description: None,
                author: None,
                language: None,
                posts_per_page: 1,
                minify: false,
                fingerprint: false,
                images: None,
                syntax_theme: crate::types::default_syntax_theme(),
                taxonomies: crate::types::default_taxonomies(),
                math: false,
                favicon: None,
                link_check_ignore: Vec::new(),
                extra: HashMap::new(),
            },
            home: None,
            pages: vec![],
            posts: vec![],
            collections,
            data: HashMap::new(),
            assets: vec![],
        };

        let output_dir = tempfile::TempDir::new().unwrap();
        let engine = ThemeEngine::new("default").unwrap();
        engine.render_site(&site, output_dir.path()).unwrap();

        assert!(output_dir.path().join("docs/index.html").exists());
        assert!(output_dir.path().join("docs/page/2/index.html").exists());
        assert!(output_dir.path().join("docs/page/3/index.html").exists());
        assert!(output_dir.path().join("docs/item-0/index.html").exists());
        assert!(output_dir.path().join("docs/item-1/index.html").exists());
        assert!(output_dir.path().join("docs/item-2/index.html").exists());
        assert!(output_dir.path().join("docs/rss.xml").exists());
        assert!(output_dir.path().join("docs/atom.xml").exists());
    }
}
