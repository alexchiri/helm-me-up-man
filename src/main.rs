use std::path::{PathBuf, Path};
use structopt::StructOpt;
use std::fs::File;
use std::io::BufReader;
use url::Url;
use serde_yaml::Value;
use tempfile::{Builder, TempDir};
use std::io::copy;
use anyhow::{Context, Result};
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use flate2::read::GzDecoder;
use tar::Archive;
use std::process::{Command, ExitStatus};
use log::{info, warn, debug, trace};

#[derive(Debug)]
struct Repo {
    name: String,
    url: Url,
    index_file: PathBuf,
}

#[derive(Debug)]
struct App {
    name: String,
    repo_name: Option<String>,
    chart_name: Option<String>,
    chart_version: String,
    values_file_path: Option<PathBuf>
}

#[derive(Debug)]
struct Helmsman {
    repos: Vec<Repo>,
    dsf_path: PathBuf,
    apps: Vec<App>,
}

#[derive(Debug, StructOpt)]
#[structopt(name = "hmum", about = "A tool to help update Helm charts and/or helmsman DSFs")]
struct Args {
    #[structopt(short = "f", long, parse(from_os_str))]
    helmsmanconfig: Option<Vec<PathBuf>>,

    #[structopt(flatten)]
    verbose: clap_verbosity_flag::Verbosity,
}

fn main() -> Result<()> {
    let args = Args::from_args();
    simple_logger::init_with_level(args.verbose.log_level().unwrap());

    let tmp_dir = Builder::new().prefix("hmum").tempdir()?;
    let helmsman_file_paths = &args.helmsmanconfig.with_context(|| "You should provide at least one helmsman config file path!")?;

    // Process all the helmsman DSFs and repos
    let mut helmsman_confs = Vec::new();

    for helmsman_file_path in helmsman_file_paths {
        let helmsman_file_path_str = &helmsman_file_path.to_str().unwrap();

        let helmsman_conf_info = get_helmsman_conf_info(&tmp_dir, &helmsman_file_path)
            .with_context(|| format!("Couldn't get helmsman info from helmsman DSF `{}`", helmsman_file_path_str))?;

        helmsman_confs.push(helmsman_conf_info);
        info!("Processed info from helmsman DSF `{}`.", helmsman_file_path_str);
    }

    for helmsman_conf in helmsman_confs {
        let helmsman_file_path_str = helmsman_conf.dsf_path.to_str().unwrap();
        debug!("Starting to go through all the apps in helmsman DSF `{}`.", helmsman_file_path_str);

        for app in helmsman_conf.apps {
            if app.repo_name.is_none() || app.chart_name.is_none() {
                debug!("App `{}` doesn't have a repo name or a chart name or both! Skipping it!", &app.name);
                continue;
            }

            let app_chart_name = &app.chart_name.unwrap();
            let app_repo_name = &app.repo_name.unwrap();
            let app_name = &app.name;
            let app_chart_version = &app.chart_version;

            let helm_repo = helmsman_conf.repos.iter().find(|repo| repo.name == *app_repo_name)
                .with_context(|| format!("Chart repo `{}` used by app `{}` in helmsman DSF `{}` is not declared!", app_repo_name, app_name, helmsman_file_path_str))?;

            let index_yaml = parse_yaml_file(&helm_repo.index_file)
                .with_context(|| format!("Failed parsing index.yaml file for repo `{}` with url `{}` from helmsman DSF file `{}`!", &helm_repo.name, &helm_repo.url.as_str(), helmsman_file_path_str))?;

            let latest_chart_info = get_latest_chart_info(&app_chart_name, &index_yaml)
                .with_context(|| format!("Could not find chart info for `{}` in index.yaml file for repo `{}` with url `{}` from helmsman DSF file `{}`!", app_chart_name, &helm_repo.name, &helm_repo.url.as_str(), helmsman_file_path_str))?;

            let latest_chart_version = latest_chart_info.get("version")
                .with_context(|| format!("Could not find the `version` property in the latest chart version for chart `{}!", app_chart_name))?;

            let latest_chart_version_str = latest_chart_version.as_str().unwrap();

            if latest_chart_version_str != app_chart_version {
                info!("There is a different version available for chart `{}`: `{}`.", app_chart_name, latest_chart_version_str);
                println!("There is a different version available for chart `{}`: `{}`.", app_chart_name, latest_chart_version_str);

                if app.values_file_path.is_some() {
                    debug!("App `{}` has a values file. Will try to update it!", app_name);
                    let latest_values_file_path = get_values_file(&tmp_dir, &latest_chart_info, helm_repo)
                        .with_context(|| format!("Couldn't retrieve the latest({}) values file for chart `{}`!", latest_chart_version_str, app_chart_name))?;

                    let original_chart_info = get_chart_info_for_version(app_chart_name, app_chart_version, &index_yaml)
                        .with_context(|| format!("Couldn't retrieve chart info for chart `{}` and version `{}`!", app_chart_name, app_chart_version))?;

                    let original_values_file_path = get_values_file(&tmp_dir, original_chart_info, helm_repo)
                        .with_context(|| format!("Couldn't retrieve original({}) values file for chart `{}`!", app_chart_version, app_chart_name))?;

                    let current_values_file_path = app.values_file_path.unwrap();
                    let current_values_file_path_str = current_values_file_path.to_str().unwrap();
                    let latest_values_file_path_str = latest_values_file_path.to_str().unwrap();
                    let original_values_file_path_str = original_values_file_path.to_str().unwrap();

                    let exit_status = merge_values_files(current_values_file_path_str, latest_values_file_path_str, original_values_file_path_str)
                        .with_context(|| format!("An error occurred while merging current values file `{}` with its original `{}` and its latest version `{}`!",
                                                 current_values_file_path_str,
                                                 original_values_file_path_str,
                                                 latest_values_file_path_str))?;

                    if exit_status.success() {
                        println!("The merge of the values files for app `{}` completed successfully!", app_chart_name);
                    } else {
                        println!("The merge of the values files for app `{}` completed with conflicts! Please review the resulting file!", app_chart_name);
                    }
                } else {
                    debug!("App `{}` doesn't have a values file.", app_name);
                    println!("App `{}` doesn't have a values file and therefore no merge will happen.", app_name);
                }

                let update_helmsman_result = update_helmsman_version(&helmsman_conf.dsf_path, app_name, app_chart_version, latest_chart_version_str);
                if update_helmsman_result.is_ok() {
                    println!("`{}` version was updated in helmsman DSF `{}` to `{}`!", app_name, &helmsman_conf.dsf_path.to_str().unwrap(), latest_chart_version_str)
                } else {
                    println!("Failed to update `{}` version in helmsman DSF `{}` to `{}`!", app_name, &helmsman_conf.dsf_path.to_str().unwrap(), latest_chart_version_str);
                    return Err(anyhow::anyhow!("Failed to update `{}` version in helmsman DSF `{}` to `{}`!", app_name, &helmsman_conf.dsf_path.to_str().unwrap(), latest_chart_version_str));
                }
            }
        }
    }

    Ok(())
}

fn get_helmsman_conf_info(tmp_dir: &TempDir, helmsman_file_path: &PathBuf) -> Result<Helmsman> {

    let mut helm_repos: Vec<Repo> = Vec::new();
    let helmsman_file_path_str = helmsman_file_path.to_str().unwrap();

    debug!("Attempting to process information from helmsman DSF `{}`.", helmsman_file_path_str);

    let helmsman_config = parse_yaml_file(&helmsman_file_path)
        .with_context(|| format!("Failed parsing helmsman DSF `{}`!", helmsman_file_path_str))?;

    // Process all the repos
    let helm_repos_value_option = helmsman_config.get("helmRepos");

    match helm_repos_value_option {
        Some(helm_repos_value) => {
            let helm_repos_conf = helm_repos_value.as_mapping()
                .with_context(|| format!("The `helmRepos` syntax in helmsman DSF `{}` is incorrect!", helmsman_file_path_str))?;

            for helm_repo_conf in helm_repos_conf.iter() {
                let helm_repo_info = get_helm_repo_info(helm_repo_conf, &tmp_dir)
                    .with_context(|| format!("Couldn't get helm repo info from helmsman DSF `{}`!", helmsman_file_path_str))?;

                helm_repos.push(helm_repo_info);
            }
        }
        None => {
            debug!("Helmsman DSF `{}` doesn't have any helm repos defined!", helmsman_file_path_str);
        }
    }

    // Process all the apps
    let mut apps: Vec<App> = Vec::new();
    let apps_conf_value = helmsman_config.get("apps")
        .with_context(|| format!("The helmsman DSF `{}` doesn't define `apps`!", helmsman_file_path_str))?;
    let apps_conf = apps_conf_value.as_mapping()
        .with_context(|| format!("The `apps` syntax in helmsman DSF `{}` is incorrect!", helmsman_file_path_str))?;
    for (index, app_conf) in apps_conf.iter().enumerate() {
        let helmsman_file_parent_path = helmsman_file_path.parent().unwrap();

        let app = get_app_info(app_conf, &helmsman_file_parent_path)
            .with_context(|| format!("Couldn't get app info from app with index `{}` in helmsman DSF `{}`", index, helmsman_file_path_str))?;

        apps.push(app);
    }

    let helmsman_info = Helmsman {
        repos: helm_repos,
        dsf_path: PathBuf::from(helmsman_file_path_str),
        apps,
    };

    trace!("Processed helmsman info: `{:?}`", helmsman_info);

    return Ok(helmsman_info);
}

fn update_helmsman_version(helmsman_file_path: &PathBuf, app_name: &str, current_app_version: &str, latest_app_version: &str) -> Result<()> {
    let helmsman_content_str = std::fs::read_to_string(helmsman_file_path).unwrap();
    debug!("Attempting to update the version for chart `{}` in helmsman DSF `{}` to `{}`.", app_name, helmsman_file_path.to_str().unwrap(), latest_app_version);
    let regex = regex::Regex::new(format!(r#"(version:[\s]*")({})(")"#, current_app_version).as_str()).unwrap();

    let version_captures = regex.captures(&helmsman_content_str).unwrap();

    if version_captures.len() == 4 {
        let updated_helmsman_content_str = regex.replace(&helmsman_content_str, format!("${{1}}{}${{3}}", latest_app_version).as_str()).to_string();
        std::fs::write(helmsman_file_path, updated_helmsman_content_str)
            .with_context(|| format!("Failed to write to the helmsman DSF `{}` to update version!", helmsman_file_path.to_str().unwrap()))?;

        debug!("Version for chart `{}` was updated successfully in helmsman DSF `{}`.", app_name, helmsman_file_path.to_str().unwrap());

        Ok(())
    } else if version_captures.len() > 4 {
        Err(anyhow::anyhow!("Found multiple matches for the version in the helmsman DSF `{}`! Didn't update the version in the helmsman DSF.", helmsman_file_path.to_str().unwrap()))
    } else {
        Err(anyhow::anyhow!("Couldn't find the version to update in the helmsman DSF `{}`! Did the file change in the meantime?", helmsman_file_path.to_str().unwrap()))
    }
}

// This function also downloads a file. That is a bad smell that I'll have to live with for now.
fn get_helm_repo_info(helm_repo_conf: (&Value, &Value), tmp_dir: &TempDir) -> Result<Repo> {
    debug!("Attempting to retrieve helm repo info.");

    let repo_name_str: String = String::from(helm_repo_conf.0.as_str().with_context(|| "Helm repo name is not a proper String!")?);

    let repo_url_str = helm_repo_conf.1.as_str()
        .with_context(|| "Helm repo URL is not a proper String!")?;

    let repo_url_str_with_slash = if repo_url_str.ends_with("/") { String::from(repo_url_str) } else { format!("{}/", repo_url_str) };
    let repo_url = Url::parse(repo_url_str_with_slash.as_str())
        .with_context(|| format!("Could not parse URL in helmsman DSF `{}`", repo_url_str_with_slash))?;
    let index_yaml_url = &repo_url.join("index.yaml")
        .with_context(|| format!("Couldn't build index.yaml url for repo `{}` with url `{}`", &repo_name_str, repo_url_str_with_slash))?;
    let index_file_path = download_file_to_temp(&tmp_dir, &index_yaml_url.as_str())
        .with_context(|| format!("Failed to download `index.yaml` file for repo `{}` from url `{}`!", &repo_name_str, &index_yaml_url))?;

    let repo_info = Repo {
        name: repo_name_str,
        url: repo_url,
        index_file: index_file_path,
    };

    trace!("Processed the following repo info: `{:?}`.", repo_info);
    return Ok(repo_info);
}

fn get_app_info(app_conf: (&Value, &Value), helmsman_conf_parent_path: &Path) -> Result<App> {
    debug!("Attempting to retrieve app info.");

    let app_name_str: String = String::from(app_conf.0.as_str().with_context(|| "The name of the app is not a proper String!")?);
    let app_conf_mapping = app_conf.1.as_mapping()
        .with_context(|| format!("The syntax of the app `{}` is incorrect!", &app_name_str))?;

    let chart_key: Value = "chart".into();
    let app_repo_chart = app_conf_mapping.get(&chart_key)
        .with_context(|| format!("App `{}` is missing the `chart` property!", &app_name_str))?;
    let app_repo_chart_str = app_repo_chart.as_str()
        .with_context(|| format!("The value of the `chart` property in app `{}` is not a proper String!", &app_name_str))?;

    let mut app_repo_name: Option<String> = None;
    let mut app_chart_name: Option<String> = None;

    if is_a_valid_chart_value(app_repo_chart_str) {
        let app_repo_chart_split = app_repo_chart_str.split("/").collect::<Vec<&str>>();
        app_repo_name = Some(String::from(app_repo_chart_split[0]));
        app_chart_name = Some(String::from(app_repo_chart_split[1]));
    }

    let chart_version_key: Value = "version".into();
    let app_chart_version = app_conf_mapping.get(&chart_version_key)
        .with_context(|| format!("App `{}` is missing the `version` property!", &app_name_str))?;
    let app_chart_version_str = app_chart_version.as_str()
        .with_context(|| format!("The value of the `version` property in app `{}` is not a proper String!", &app_name_str))?;

    let mut app_values_file: Option<&Value> = None;
    let values_file_key: Value = "valuesFile".into();
    let app_values_file_option = app_conf_mapping.get(&values_file_key);

    match app_values_file_option {
        Some(values_file) => {
            trace!("Found `valuesFile` for app {}", &app_name_str);
            app_values_file = Some(values_file);
        }
        None => {
            trace!("App {} is missing `valuesFile` property. Trying to find `valuesFiles`.", &app_name_str);
            let values_files_key = "valuesFiles".into();
            let app_values_files_option = app_conf_mapping.get(&values_files_key);

            match app_values_files_option {
                Some(values_files) => {
                    trace!("Found `valuesFiles` for app {}", &app_name_str);
                    let values_files_seq = values_files.as_sequence()
                        .with_context(|| format!("`valuesFiles` is not an array for app `{}`!", &app_name_str))?;
                    app_values_file = Some(values_files_seq.first().with_context(|| format!("`valuesFiles` is empty for app `{}`", &app_name_str))?);
                }
                None => {
                    trace!("App {} is missing `valuesFile` and `valuesFiles` properties.", &app_name_str);
                }
            }
        }
    }

    let mut values_file_path: Option<PathBuf> = None;
    match app_values_file {
        Some(path_value) => {
            let app_values_file_relative_path_str = path_value.as_str()
                .with_context(|| format!("The value of the `valuesFile` property in app `{}` is not a proper String!", &app_name_str))?;

            let full_values_file_path = helmsman_conf_parent_path.join(app_values_file_relative_path_str);
            if full_values_file_path.exists() {
                values_file_path = Some(full_values_file_path)
            } else {
                debug!("Values file path `{}` for app `{}` doesn't exist!", full_values_file_path.to_str().unwrap(), &app_name_str);
            }

        }
        None => {}
    }



    let app_info = App {
        name: app_name_str,
        repo_name: app_repo_name,
        chart_name: app_chart_name,
        chart_version: String::from(app_chart_version_str),
        values_file_path,
    };

    trace!("Processed the following app info: `{:?}`.", app_info);

    return Ok(app_info);
}

fn is_a_valid_chart_value(chart_value: &str) -> bool {
    let regex = regex::Regex::new(r"^[\w-]+/[\w-]+$").unwrap();
    return regex.is_match(chart_value);
}

fn merge_values_files(current_values_file_path_str: &str, latest_values_file_path_str: &str, original_values_file_path_str: &str) -> Result<ExitStatus> {
    debug!("Attempting to merge current values file `{}`, with the original `{}` and the latest `{}`.", current_values_file_path_str, original_values_file_path_str, latest_values_file_path_str);

    let exit_status = Command::new("git")
        .arg("merge-file")
        .arg(current_values_file_path_str)
        .arg(original_values_file_path_str)
        .arg(latest_values_file_path_str)
        .status()
        .with_context(|| format!("Error happened while merging current values file `{}` with its original `{}` and its latest version `{}`!",
                                 current_values_file_path_str,
                                 original_values_file_path_str,
                                 latest_values_file_path_str))?;

    debug!("Merge completed without exceptions.");
    return Ok(exit_status);
}

fn get_chart_info_for_version<'a>(chart_name: &str, chart_version: &str, index_yaml_content: &'a Value) -> Result<&'a Value> {
    debug!("Attempting to process chart info for chart `{}` and version `{}`.", chart_name, chart_version);

    let entries_value = index_yaml_content.get("entries")
        .with_context(|| "The index.yaml file doesn't have `entries`!")?;

    let chart_versions = entries_value.get(chart_name)
        .with_context(|| format!("Couldn't find chart `{}` in index.yaml file!", chart_name))?;

    let chart_versions_seq = chart_versions.as_sequence()
        .with_context(|| format!("The syntax of the chart entries for chart `{}` is incorrect!", chart_name))?;

    let chart_info_version_filter = |chart_info: &&Value| {
        let version = chart_info.get("version").unwrap();

        version == chart_version
    };

    let chart_info = chart_versions_seq.iter().find(chart_info_version_filter)
        .with_context(|| format!("Could not find the chart version `{}` for chart `{}`", chart_version, chart_name))?;

    trace!("Retrieved information for chart `{}` and version `{}`: `{:?}`.", chart_name, chart_version, chart_info);

    return Ok(chart_info);
}


fn get_values_file(tmp_dir: &TempDir, chart_info: &Value, repo: &Repo) -> Result<PathBuf> {
    let chart_name = chart_info.get("name")
        .with_context(|| "Couldn't find property `name` in chart info!")?.as_str().unwrap();
    debug!("Retrieving values file for chart `{}`", chart_name);

    let chart_urls_seq = chart_info.get("urls")
        .with_context(|| "Could not find the `urls` property in the latest chart version!")?
        .as_sequence().unwrap();
    let chart_url_str = chart_urls_seq.first()
        .with_context(|| "Could not retrieve the latest url for chart!")?
        .as_str().unwrap();

    let mut chart_url = Url::parse(chart_url_str);
    match chart_url {
        Err(e) => {
            info!("It seems that the chart URL {} could not be parsed: {}. This might be a relative URL, so will attempt to appent it to the chart repo URL.", chart_url_str, e);

            let latest_chart_absolute_url = repo.url.join(chart_url_str)
                .with_context(|| format!("The URL provided in the chart is not an absolute or a relative URL: {}", chart_url_str))?;
            chart_url = Ok(latest_chart_absolute_url);
        }

        Ok(_) => {
            debug!("Chart URL is valid `{}`", chart_url_str);
        }
    }

    let chart_archive_path = download_chart_archive(&tmp_dir, chart_url.unwrap().as_str())
        .with_context(|| "Couldn't download the latest chart archive!")?;

    let chart_archive_path_str = chart_archive_path.to_str().unwrap();
    let chart_untared_path = untar_archive(&chart_archive_path, &tmp_dir)
        .with_context(|| format!("Failed to untar the chart archive `{}`!", &chart_archive_path_str))?;
    let chart_values_file_path = chart_untared_path.join(format!("{}/values.yaml", chart_name));

    debug!("Values file was downloaded successfully to `{}`", chart_values_file_path.to_str().unwrap());
    return Ok(chart_values_file_path);
}

fn download_chart_archive(tmp_dir: &TempDir, latest_chart_url_str: &str) -> Result<PathBuf> {
    debug!("Attempting to download chart from `{}`", latest_chart_url_str);
    let latest_chart_archive_path = download_file_to_temp(&tmp_dir, latest_chart_url_str)
        .with_context(|| format!("Failed to download chart archive from `{}`!", latest_chart_url_str))?;

    return Ok(latest_chart_archive_path);
}

fn get_latest_chart_info<'a>(chart_name: &str, index_yaml_content: &'a Value) -> Result<&'a Value> {
    debug!("Attempting to retrieve latest chart information for chart `{}` from repo index.yaml file.", chart_name);
    let entries_value = index_yaml_content.get("entries")
        .with_context(|| "The index.yaml file doesn't have `entries`!")?;

    let chart_versions = entries_value.get(chart_name)
        .with_context(|| format!("Couldn't find chart `{}` in index.yaml file!", chart_name))?;

    let chart_versions_seq = chart_versions.as_sequence()
        .with_context(|| format!("The syntax of the chart entries for chart `{}` is incorrect!", chart_name))?;

    let latest_chart_info = chart_versions_seq.first()
        .with_context(|| format!("Could not get the latest chart entry for the chart `{}`!", chart_name))?;

    trace!("Retrieved latest chart information for chart `{}`: `{:?}`", chart_name, latest_chart_info);

    Ok(latest_chart_info)
}

fn parse_yaml_file(file_path: &PathBuf) -> Result<Value> {
    let file_path_str = file_path.to_str().unwrap();

    debug!("Attempting to parse yaml file `{}`.", file_path_str);

    let file = File::open(file_path)
        .with_context(|| format!("Could not open file `{}`", file_path_str))?;
    let file_reader = BufReader::new(file);
    let file_content: Value = serde_yaml::from_reader(file_reader).with_context(|| "Could not parse yaml file!")?;

    debug!("File `{}` was parsed successfully!", file_path_str);
    Ok(file_content)
}

fn download_file_to_temp(tmp_dir: &TempDir, target: &str) -> Result<PathBuf> {
    debug!("Attempting to download file from `{}` to temporary folder.", target);
    let response = ureq::get(target).call();
    let temp_file_path = tmp_dir.path().join(generate_rand_filename());

    return if response.ok() {
        let mut temp_file = File::create(&temp_file_path)
            .with_context(|| format!("An error occurred while creating a tempfile in folder `{}`", tmp_dir.path().display()))?;

        copy(&mut response.into_reader(), &mut temp_file)?;

        debug!("File was downloaded successfully to `{}`.", temp_file_path.to_str().unwrap());
        Ok(temp_file_path)
    } else {
        Err(anyhow::anyhow!("Fetching the file failed with `{}`!", &response.status_line()))
    };
}

fn generate_rand_filename() -> String {
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(30)
        .collect();

    return rand_string;
}

fn untar_archive(path: &PathBuf, tmp_dir: &TempDir) -> Result<PathBuf> {
    let path_str = path.to_str().unwrap();
    let tar_gz = File::open(path)
        .with_context(|| format!("Couldn't open file `{}`!", path_str))?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);

    let extraction_path = tmp_dir.path().join(generate_rand_filename());
    let extraction_path_str = &extraction_path.to_str().unwrap();

    archive.unpack(&extraction_path)
        .with_context(|| format!("Failed to extract archive `{}` to `{}`!", path_str, extraction_path_str))?;

    return Ok(extraction_path);
}
