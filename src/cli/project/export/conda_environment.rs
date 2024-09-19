use std::path::PathBuf;

use clap::Parser;
use itertools::Itertools;
use miette::{Context, IntoDiagnostic};
use pep508_rs::ExtraName;
use pixi_manifest::{
    pypi::{PyPiPackageName, VersionOrStar},
    FeaturesExt, HasFeaturesIter, PyPiRequirement,
};
use rattler_conda_types::{
    EnvironmentYaml, MatchSpec, MatchSpecOrSubSection, ParseStrictness, Platform,
};
use rattler_lock::FindLinksUrlOrPath;

use crate::project::Environment;
use crate::Project;

#[derive(Debug, Parser)]
pub struct Args {
    /// Explicit path to export the environment to
    pub output_path: Option<PathBuf>,

    /// The platform to render the environment file for.
    /// Defaults to the current platform.
    #[arg(short, long)]
    pub platform: Option<Platform>,

    /// The environment to render the environment file for.
    /// Defaults to the default environment.
    #[arg(short, long)]
    pub environment: Option<String>,
}

fn format_pip_extras(extras: &[ExtraName]) -> String {
    if extras.is_empty() {
        return String::new();
    }
    format!(
        "[{}]",
        extras.iter().map(|extra| format!("{extra}")).join("")
    )
}

fn format_pip_dependency(name: &PyPiPackageName, requirement: &PyPiRequirement) -> String {
    match requirement {
        PyPiRequirement::Git {
            url: git_url,
            extras,
        } => {
            let mut git_string = format!(
                "{name}{extras} @ git+{url}",
                name = name.as_normalized(),
                extras = format_pip_extras(extras),
                url = git_url.git,
            );

            if let Some(ref branch) = git_url.branch {
                git_string.push_str(&format!("@{branch}"));
            } else if let Some(ref tag) = git_url.tag {
                git_string.push_str(&format!("@{tag}"));
            } else if let Some(ref rev) = git_url.rev {
                git_string.push_str(&format!("@{rev}"));
            }

            if let Some(ref subdirectory) = git_url.subdirectory {
                git_string.push_str(&format!("#subdirectory=={subdirectory}"));
            }

            git_string
        }
        PyPiRequirement::Path {
            path,
            editable,
            extras,
        } => {
            if let Some(_editable) = editable {
                format!(
                    "-e {path}{extras}",
                    path = path.to_string_lossy(),
                    extras = format_pip_extras(extras),
                )
            } else {
                format!(
                    "{path}{extras}",
                    path = path.to_string_lossy(),
                    extras = format_pip_extras(extras),
                )
            }
        }
        PyPiRequirement::Url {
            url,
            subdirectory,
            extras,
        } => {
            let mut url_string = format!(
                "{name}{extras} @ {url}",
                name = name.as_normalized(),
                extras = format_pip_extras(extras),
                url = url,
            );

            if let Some(ref subdirectory) = subdirectory {
                url_string.push_str(&format!("#subdirectory=={subdirectory}"));
            }

            url_string
        }
        PyPiRequirement::Version { version, extras } => {
            format!(
                "{name}{extras}{version}",
                name = name.as_normalized(),
                extras = format_pip_extras(extras),
                version = version
            )
        }
        PyPiRequirement::RawVersion(version) => match version {
            VersionOrStar::Version(_) => format!(
                "{name}{version}",
                name = name.as_normalized(),
                version = version
            ),
            VersionOrStar::Star => format!("{name}", name = name.as_normalized()),
        },
    }
}

fn build_env_yaml(
    platform: &Platform,
    environment: &Environment,
) -> miette::Result<EnvironmentYaml> {
    let mut env_yaml = rattler_conda_types::EnvironmentYaml {
        name: Some(environment.name().as_str().to_string()),
        channels: environment.channels().into_iter().cloned().collect_vec(),
        ..Default::default()
    };

    let mut pip_dependencies: Vec<String> = Vec::new();

    for feature in environment.features() {
        if let Some(dependencies) = feature.dependencies(None, Some(*platform)) {
            for (key, value) in dependencies.iter() {
                let spec = MatchSpec {
                    name: Some(key.clone()),
                    version: value.clone().into_version(),
                    build: None,
                    build_number: None,
                    subdir: None,
                    md5: None,
                    sha256: None,
                    url: None,
                    file_name: None,
                    channel: None,
                    namespace: None,
                };
                env_yaml
                    .dependencies
                    .push(MatchSpecOrSubSection::MatchSpec(spec));
            }
        }

        if feature.has_pypi_dependencies() {
            if let Some(pypi_dependencies) = feature.pypi_dependencies(Some(*platform)) {
                for (name, requirement) in pypi_dependencies.iter() {
                    pip_dependencies.push(format_pip_dependency(name, requirement));
                }
            }
        }
    }

    if !pip_dependencies.is_empty() {
        let pypi_options = environment.pypi_options();
        if let Some(ref find_links) = pypi_options.find_links {
            for find_link in find_links {
                match find_link {
                    FindLinksUrlOrPath::Url(url) => {
                        pip_dependencies.insert(0, format!("--find-links {url}"));
                    }
                    FindLinksUrlOrPath::Path(path) => {
                        pip_dependencies
                            .insert(0, format!("--find-links {}", path.to_string_lossy()));
                    }
                }
            }
        }
        if let Some(ref extra_index_urls) = pypi_options.extra_index_urls {
            for extra_index_url in extra_index_urls {
                pip_dependencies.insert(0, format!("--extra-index-url {extra_index_url}"));
            }
        }
        if let Some(ref index_url) = pypi_options.index_url {
            pip_dependencies.insert(0, format!("--index-url {index_url}"));
        }

        env_yaml.dependencies.push(MatchSpecOrSubSection::MatchSpec(
            MatchSpec::from_str("pip", ParseStrictness::Lenient).unwrap(),
        ));

        env_yaml
            .dependencies
            .push(MatchSpecOrSubSection::SubSection(
                "pip".to_string(),
                pip_dependencies.into_iter().collect_vec(),
            ));
    }

    Ok(env_yaml)
}

pub async fn execute(project: Project, args: Args) -> miette::Result<()> {
    let environment = project.environment_from_name_or_env_var(args.environment)?;
    let platform = args.platform.unwrap_or_else(|| environment.best_platform());

    let env_yaml = build_env_yaml(&platform, &environment).unwrap();

    if let Some(output_path) = args.output_path {
        env_yaml
            .to_path(output_path.as_path())
            .into_diagnostic()
            .with_context(|| "failed to write environment YAML")?;
    } else {
        println!("{}", env_yaml.to_yaml_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    #[test]
    fn test_export_conda_env_yaml() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/cli/project/export/test-data/testenv/pixi.toml");
        let project = Project::from_path(&path).unwrap();
        let args = Args {
            output_path: None,
            platform: Some(Platform::Osx64),
            environment: Some("default".to_string()),
        };
        let environment = project
            .environment_from_name_or_env_var(args.environment)
            .unwrap();
        let platform = args.platform.unwrap_or_else(|| environment.best_platform());

        let env_yaml = build_env_yaml(&platform, &environment);
        insta::assert_snapshot!(
            "test_export_conda_env_yaml",
            env_yaml.unwrap().to_yaml_string()
        );
    }

    #[test]
    fn test_export_conda_env_yaml_with_pip_extras() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/pypi/pixi.toml");
        let project = Project::from_path(&path).unwrap();
        let args = Args {
            output_path: None,
            platform: None,
            environment: Some("default".to_string()),
        };
        let environment = project
            .environment_from_name_or_env_var(args.environment)
            .unwrap();
        let platform = args.platform.unwrap_or_else(|| environment.best_platform());

        let env_yaml = build_env_yaml(&platform, &environment);
        insta::assert_snapshot!(
            "test_export_conda_env_yaml_with_pip_extras",
            env_yaml.unwrap().to_yaml_string()
        );
    }

    #[test]
    fn test_export_conda_env_yaml_with_pip_source_editable() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/pypi-source-deps/pixi.toml");
        let project = Project::from_path(&path).unwrap();
        let args = Args {
            output_path: None,
            platform: None,
            environment: Some("default".to_string()),
        };
        let environment = project
            .environment_from_name_or_env_var(args.environment)
            .unwrap();
        let platform = args.platform.unwrap_or_else(|| environment.best_platform());

        let env_yaml = build_env_yaml(&platform, &environment);
        insta::assert_snapshot!(
            "test_export_conda_env_yaml_with_source_editable",
            env_yaml.unwrap().to_yaml_string()
        );
    }

    #[test]
    fn test_export_conda_env_yaml_with_pip_custom_registry() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/pypi-custom-registry/pixi.toml");
        let project = Project::from_path(&path).unwrap();
        let args = Args {
            output_path: None,
            platform: None,
            environment: Some("alternative".to_string()),
        };
        let environment = project
            .environment_from_name_or_env_var(args.environment)
            .unwrap();
        let platform = args.platform.unwrap_or_else(|| environment.best_platform());

        let env_yaml = build_env_yaml(&platform, &environment);
        insta::assert_snapshot!(
            "test_export_conda_env_yaml_with_pip_custom_registry",
            env_yaml.unwrap().to_yaml_string()
        );
    }

    #[test]
    fn test_export_conda_env_yaml_with_pip_find_links() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/pypi-find-links/pixi.toml");
        let project = Project::from_path(&path).unwrap();
        let args = Args {
            output_path: None,
            platform: None,
            environment: Some("default".to_string()),
        };
        let environment = project
            .environment_from_name_or_env_var(args.environment)
            .unwrap();
        let platform = args.platform.unwrap_or_else(|| environment.best_platform());

        let env_yaml = build_env_yaml(&platform, &environment);
        insta::assert_snapshot!(
            "test_export_conda_env_yaml_with_pip_find_links",
            env_yaml.unwrap().to_yaml_string()
        );
    }

    #[test]
    fn test_export_conda_env_yaml_pyproject_panic() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/docker/pyproject.toml");
        let project = Project::from_path(&path).unwrap();
        let args = Args {
            output_path: None,
            platform: Some(Platform::OsxArm64),
            environment: Some("default".to_string()),
        };
        let environment = project
            .environment_from_name_or_env_var(args.environment)
            .unwrap();
        let platform = args.platform.unwrap_or_else(|| environment.best_platform());

        let env_yaml = build_env_yaml(&platform, &environment);
        insta::assert_snapshot!(
            "test_export_conda_env_yaml_pyproject_panic",
            env_yaml.unwrap().to_yaml_string()
        );
    }
}