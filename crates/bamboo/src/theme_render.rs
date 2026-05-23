//! Per-target rendering helpers used by [`ThemeEngine::render_site`](crate::ThemeEngine::render_site).
//! Each function writes one logical group of output files (index, single page,
//! single post, pagination pages, collection pages, 404, search shell) using a
//! preloaded Tera instance.

use crate::error::Result;
use crate::types::{Asset, Collection, CollectionItem, Page, Post, Site, SiteConfig};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tera::{Context, Tera};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SiteMetadata<'a> {
    config: &'a SiteConfig,
    pages: &'a [Page],
    posts: &'a [Post],
    data: &'a HashMap<String, serde_json::Value>,
    collections: &'a HashMap<String, Collection>,
}

pub(crate) fn site_metadata(site: &Site) -> SiteMetadata<'_> {
    SiteMetadata {
        config: &site.config,
        pages: &site.pages,
        posts: &site.posts,
        data: &site.data,
        collections: &site.collections,
    }
}

fn related_posts<'a>(site: &'a Site, post: &Post, limit: usize) -> Vec<&'a Post> {
    if post.tags.is_empty() && post.categories.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, &Post)> = site
        .posts
        .iter()
        .filter(|candidate| candidate.content.slug != post.content.slug)
        .map(|candidate| {
            let shared_tags = candidate
                .tags
                .iter()
                .filter(|tag| post.tags.contains(tag))
                .count();
            let shared_categories = candidate
                .categories
                .iter()
                .filter(|category| post.categories.contains(category))
                .count();
            (shared_tags + shared_categories, candidate)
        })
        .filter(|(score, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.date.cmp(&a.1.date)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, candidate)| candidate)
        .collect()
}

pub(crate) fn render_index(tera: &Tera, site: &Site, output_dir: &Path) -> Result<()> {
    let posts_per_page = site.config.posts_per_page;
    let index_posts: Vec<&Post> = site.posts.iter().take(posts_per_page).collect();
    let total_pages = if posts_per_page > 0 && !site.posts.is_empty() {
        site.posts.len().div_ceil(posts_per_page)
    } else {
        1
    };
    let base_url = site.config.base_url.trim_end_matches('/');

    let mut context = Context::new();
    let metadata = site_metadata(site);
    context.insert("site", &metadata);
    context.insert("posts", &index_posts);
    context.insert("current_page", &1usize);
    context.insert("total_pages", &total_pages);

    if total_pages > 1 {
        let next_url = format!("{}/page/2/", base_url);
        context.insert("next_page_url", &next_url);
    }

    let template_name = if let Some(home) = &site.home {
        context.insert("home", home);
        context.insert("page", home);
        home.content.template.as_deref().unwrap_or("index.html")
    } else {
        "index.html"
    };

    let rendered = tera.render(template_name, &context)?;
    fs::write(output_dir.join("index.html"), rendered)?;

    Ok(())
}

pub(crate) fn render_page(tera: &Tera, site: &Site, page: &Page, output_dir: &Path) -> Result<()> {
    let mut context = Context::new();
    let metadata = site_metadata(site);
    context.insert("site", &metadata);
    context.insert("page", page);
    let math = site.config.math || page.content.frontmatter.get_bool("math").unwrap_or(false);
    context.insert("math", &math);

    let template_name = page.content.template.as_deref().unwrap_or("page.html");
    let rendered = tera.render(template_name, &context)?;

    let output_path = output_dir.join(&page.content.path);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(output_path, rendered)?;

    Ok(())
}

pub(crate) fn render_post(
    tera: &Tera,
    site: &Site,
    post: &Post,
    prev_post: Option<&Post>,
    next_post: Option<&Post>,
    output_dir: &Path,
) -> Result<()> {
    let mut context = Context::new();
    let metadata = site_metadata(site);
    context.insert("site", &metadata);
    context.insert("post", post);
    let math = site.config.math || post.content.frontmatter.get_bool("math").unwrap_or(false);
    context.insert("math", &math);

    if let Some(prev) = prev_post {
        context.insert("prev_post", prev);
    }
    if let Some(next) = next_post {
        context.insert("next_post", next);
    }

    let related = related_posts(site, post, 3);
    context.insert("related_posts", &related);

    let template_name = post.content.template.as_deref().unwrap_or("post.html");
    let rendered = tera.render(template_name, &context)?;

    let output_path = output_dir.join(&post.content.path);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(output_path, rendered)?;

    Ok(())
}

pub(crate) fn render_pagination(tera: &Tera, site: &Site, output_dir: &Path) -> Result<()> {
    let posts_per_page = site.config.posts_per_page;
    if posts_per_page == 0 || site.posts.is_empty() {
        return Ok(());
    }

    let total_pages = site.posts.len().div_ceil(posts_per_page);
    let base_url = site.config.base_url.trim_end_matches('/');
    let metadata = site_metadata(site);

    for page_number in 2..=total_pages {
        let start = (page_number - 1) * posts_per_page;
        let end = (start + posts_per_page).min(site.posts.len());
        let page_posts = &site.posts[start..end];

        let mut context = Context::new();
        context.insert("site", &metadata);
        context.insert("posts", page_posts);
        context.insert("current_page", &page_number);
        context.insert("total_pages", &total_pages);

        let prev_url = if page_number == 2 {
            format!("{}/", base_url)
        } else {
            format!("{}/page/{}/", base_url, page_number - 1)
        };
        context.insert("prev_page_url", &prev_url);

        if page_number < total_pages {
            let next_url = format!("{}/page/{}/", base_url, page_number + 1);
            context.insert("next_page_url", &next_url);
        }

        let rendered = tera.render("pagination.html", &context)?;
        let page_dir = output_dir.join("page").join(page_number.to_string());
        fs::create_dir_all(&page_dir)?;
        fs::write(page_dir.join("index.html"), rendered)?;
    }

    Ok(())
}

pub(crate) fn render_404(tera: &Tera, site: &Site, output_dir: &Path) -> Result<()> {
    let mut context = Context::new();
    let metadata = site_metadata(site);
    context.insert("site", &metadata);

    let four_oh_four_page = site.pages.iter().find(|page| page.content.slug == "404");
    if let Some(page) = four_oh_four_page {
        context.insert("page", page);
    }

    let rendered = tera.render("404.html", &context)?;
    fs::write(output_dir.join("404.html"), rendered)?;

    Ok(())
}

pub(crate) fn render_search(tera: &Tera, site: &Site, output_dir: &Path) -> Result<()> {
    let search_dir = output_dir.join("search");
    let search_index = search_dir.join("index.html");

    let mut context = Context::new();
    let metadata = site_metadata(site);
    context.insert("site", &metadata);

    let rendered = tera.render("search.html", &context)?;
    fs::create_dir_all(&search_dir)?;
    fs::write(search_index, rendered)?;

    Ok(())
}

pub(crate) fn render_collection(
    tera: &Tera,
    site: &Site,
    name: &str,
    collection: &Collection,
    output_dir: &Path,
) -> Result<()> {
    let metadata = site_metadata(site);
    let items_per_page = site.config.posts_per_page;
    let base_url = site.config.base_url.trim_end_matches('/');

    let effective_per_page = if items_per_page == 0 {
        collection.items.len().max(1)
    } else {
        items_per_page
    };

    let total_pages = if collection.items.is_empty() {
        1
    } else {
        collection.items.len().div_ceil(effective_per_page)
    };

    for page_number in 1..=total_pages {
        let start = (page_number - 1) * effective_per_page;
        let end = (start + effective_per_page).min(collection.items.len());
        let page_items = &collection.items[start..end];

        let mut context = Context::new();
        context.insert("site", &metadata);
        context.insert("collection", collection);
        context.insert("collection_name", name);
        context.insert("items", &page_items);
        context.insert("current_page", &page_number);
        context.insert("total_pages", &total_pages);

        if page_number > 1 {
            let prev_url = if page_number == 2 {
                format!("{}/{}/", base_url, name)
            } else {
                format!("{}/{}/page/{}/", base_url, name, page_number - 1)
            };
            context.insert("prev_page_url", &prev_url);
        }

        if page_number < total_pages {
            let next_url = format!("{}/{}/page/{}/", base_url, name, page_number + 1);
            context.insert("next_page_url", &next_url);
        }

        let rendered = tera.render("collection.html", &context)?;

        if page_number == 1 {
            let index_path = output_dir.join(name).join("index.html");
            if let Some(parent) = index_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(index_path, rendered)?;
        } else {
            let page_dir = output_dir
                .join(name)
                .join("page")
                .join(page_number.to_string());
            fs::create_dir_all(&page_dir)?;
            fs::write(page_dir.join("index.html"), rendered)?;
        }
    }

    for item in &collection.items {
        render_collection_item(tera, site, name, collection, item, output_dir)?;
    }

    Ok(())
}

fn render_collection_item(
    tera: &Tera,
    site: &Site,
    collection_name: &str,
    collection: &Collection,
    item: &CollectionItem,
    output_dir: &Path,
) -> Result<()> {
    let mut context = Context::new();
    let metadata = site_metadata(site);
    context.insert("site", &metadata);
    context.insert("item", item);
    context.insert("collection", collection);
    context.insert("collection_name", collection_name);
    let math = site.config.math || item.content.frontmatter.get_bool("math").unwrap_or(false);
    context.insert("math", &math);

    let template_name = item
        .content
        .template
        .as_deref()
        .unwrap_or("collection_item.html");

    let template_name = if tera.get_template_names().any(|name| name == template_name) {
        template_name
    } else {
        context.insert("page", item);
        "page.html"
    };

    let rendered = tera.render(template_name, &context)?;
    let output_path = output_dir.join(&item.content.path);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, rendered)?;

    Ok(())
}

pub(crate) fn copy_assets(assets: &[Asset], output_dir: &Path) -> Result<()> {
    for asset in assets {
        let dest = output_dir.join(&asset.dest);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&asset.source, &dest)?;
    }

    Ok(())
}
