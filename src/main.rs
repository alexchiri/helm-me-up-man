use std::path::PathBuf;
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
use std::process::Command;

#[derive(Debug)]
struct Repo {
    name: String,
    url: Url,
    index_file: PathBuf
}

#[derive(Debug)]
struct App {
    name: String,
    repo_name: String,
    chart_name: String,
    chart_version: String,
    values_file_path: PathBuf,
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
}

fn main() -> Result<()> {
    let args = Args::from_args();
    println!("{:?}", args);

    let tmp_dir = Builder::new().prefix("hmum").tempdir()?;
    let helmsman_file_paths = &args.helmsmanconfig.expect("You should provide at least one helmsman config file path!");

    // Process all the helmsman DSFs and repos
    let mut helmsman_confs = Vec::new();

    for helmsman_file_path in helmsman_file_paths {
        let mut helm_repos: Vec<Repo> = Vec::new();

        println!("{:?}", helmsman_file_path);
        let helmsman_file_path_str = helmsman_file_path.to_str().unwrap();
        let helmsman_config = parse_yaml_file(&helmsman_file_path)
            .with_context(|| format!("Failed parsing helmsman DSF `{}`!", helmsman_file_path_str))?;
        println!("{:?}", helmsman_config);

        // Process all the repos
        let helm_repos_value = helmsman_config.get("helmRepos").expect(&format!("The helmsman DSF `{}` doesn't define `helmRepos`!", helmsman_file_path_str));
        let helm_repos_conf = helm_repos_value.as_mapping().expect(&format!("The `helmRepos` syntax in helmsman DSF `{}` is incorrect!", helmsman_file_path_str));

        for helm_repo_conf in helm_repos_conf.iter() {
            let repo_name_str: String = String::from(helm_repo_conf.0.as_str().expect(&format!("Helm repo name is not a proper String in `{}`", helmsman_file_path_str)));
            let repo_url_str = helm_repo_conf.1.as_str().expect(&format!("Helm repo URL is not a proper String in `{}`", helmsman_file_path_str));
            let repo_url = Url::parse(repo_url_str).with_context(|| format!("Could not parse URL in helmsman DSF `{}`", repo_url_str))?;

            let index_yaml_url = &repo_url.join("index.yaml")
                .with_context(|| format!("Couldn't build index.yaml url for repo `{}` with url `{}`", &repo_name_str, repo_url_str))?;

            let index_file_path = download_file_to_temp(&tmp_dir, &index_yaml_url.as_str())
                .with_context(|| format!("Failed to download `index.yaml` file for repo `{}` from url `{}`!", &repo_name_str, &index_yaml_url))?;

            helm_repos.push(Repo {
                name: repo_name_str,
                url: repo_url,
                index_file: index_file_path
            });
        }

        println!("{:?}", helm_repos);

        // Process all the apps
        let mut apps: Vec<App> = Vec::new();
        let apps_conf_value = helmsman_config.get("apps").expect(&format!("The helmsman DSF `{}` doesn't define `apps`!", helmsman_file_path_str));
        let apps_conf = apps_conf_value.as_mapping().expect(&format!("The `apps` syntax in helmsman DSF `{}` is incorrect!", helmsman_file_path_str));

        for (index, app_conf) in apps_conf.iter().enumerate() {
            let app_name_str: String = String::from(app_conf.0.as_str()
                .expect(&format!("The name of the app with index `{}` is not a proper String in `{}`", index, helmsman_file_path_str)));
            let app_conf_mapping = app_conf.1.as_mapping()
                .expect(&format!("The syntax of the app `{}` in helmsman DSF `{}` is incorrect!", &app_name_str, helmsman_file_path_str));

            let chart_key: Value = "chart".into();
            let app_repo_chart = app_conf_mapping.get(&chart_key)
                .expect(&format!("App `{}` is missing the `chart` property in helmsman DSF `{}`", &app_name_str, helmsman_file_path_str));
            let app_repo_chart_str = app_repo_chart.as_str()
                .expect(&format!("The value of the `chart` property in app `{}` in helmsman DSF `{}` is not a proper String!", &app_name_str, helmsman_file_path_str));
            let app_repo_chart_split = app_repo_chart_str.split("/").collect::<Vec<&str>>();
            let app_repo_name = app_repo_chart_split[0];
            let app_chart_name = app_repo_chart_split[1];

            let chart_version_key: Value = "version".into();
            let app_chart_version = app_conf_mapping.get(&chart_version_key)
                .expect(&format!("App `{}` is missing the `version` property in helmsman DSF `{}`", &app_name_str, helmsman_file_path_str));
            let app_chart_version_str = app_chart_version.as_str()
                .expect(&format!("The value of the `version` property in app `{}` in helmsman DSF `{}` is not a proper String!", &app_name_str, helmsman_file_path_str));

            let chart_values_key: Value = "valuesFile".into();
            let app_values_file = app_conf_mapping.get(&chart_values_key)
                .expect(&format!("App `{}` is missing the `valuesFile` property in helmsman DSF `{}`", &app_name_str, helmsman_file_path_str));
            let app_values_file_str = app_values_file.as_str()
                .expect(&format!("The value of the `valuesFile` property in app `{}` in helmsman DSF `{}` is not a proper String!", &app_name_str, helmsman_file_path_str));

            apps.push(App {
                name: app_name_str,
                repo_name: String::from(app_repo_name),
                chart_name: String::from(app_chart_name),
                chart_version: String::from(app_chart_version_str),
                values_file_path: PathBuf::from(app_values_file_str),
            });
        }

        println!("{:?}", apps);

        helmsman_confs.push(Helmsman {
            repos: helm_repos,
            dsf_path: PathBuf::from(helmsman_file_path_str),
            apps,
        });
    }

    println!("{:?}", helmsman_confs);


    for helmsman_conf in helmsman_confs {
        let helmsman_file_path_str = helmsman_conf.dsf_path.to_str().unwrap();

        for app in helmsman_conf.apps {
            let helm_repo = helmsman_conf.repos.iter().find(|repo| repo.name == app.repo_name)
                .expect(&format!("Chart repo `{}` used by app `{}` in helmsman DSF `{}` is not declared!", &app.repo_name, &app.name, helmsman_file_path_str));

            println!("{:?}", &helm_repo.index_file);
            std::thread::sleep(std::time::Duration::from_secs(30));

            let index_yaml = parse_yaml_file(&helm_repo.index_file)
                .with_context(|| format!("Failed parsing index.yaml file for repo `{}` with url `{}` from helmsman DSF file `{}`!", &helm_repo.name, &helm_repo.url.as_str(), helmsman_file_path_str))?;

            let latest_chart_info = get_latest_chart_info_from_index(&app.chart_name, &index_yaml)
                .with_context(|| format!("Could not find chart info for `{}` in index.yaml file for repo `{}` with url `{}` from helmsman DSF file `{}`!", &app.chart_name, &helm_repo.name, &helm_repo.url.as_str(), helmsman_file_path_str))?;

            println!("Latest chart info {:?}", latest_chart_info);

            let latest_chart_version = latest_chart_info.get("version")
                .with_context(|| format!("Could not find the `version` property in the latest chart version for chart `{}!", &app.chart_name))?;

            let latest_chart_version_str = latest_chart_version.as_str().unwrap();

            if latest_chart_version_str != &app.chart_version {
                let latest_chart_values_file_path = get_latest_version_values_file(&tmp_dir, &latest_chart_info)
                    .with_context(|| format!("Couldn't retrieve the latest values file for chart `{}`", &app.chart_name))?;

                let current_chart_values_file_path = get_current_version_values_file(&tmp_dir, &app.chart_name, &app.chart_version, &index_yaml)?;

//                let current_chart_archive_path =



//                Command::new("git")
//                    .arg("merge-file")
//                    .arg(app.values_file_path.to_str().unwrap())
//                    .
                println!("{:?}", latest_chart_values_file_path);
            }
        }
    }

    Ok(())
}

fn get_current_version_values_file(tmp_dir: &TempDir, chart_name: &str, chart_version: &str, index_yaml_content: &Value) -> Result<PathBuf> {
    let current_version_chart_info = get_current_version_chart_info(chart_name, chart_version, index_yaml_content)?;
}

fn get_current_version_chart_info<'a>(chart_name: &str, chart_version: &str, index_yaml_content: &'a Value) -> Result<&'a Value> {
    let entries_value = index_yaml_content.get("entries")
        .with_context(|| "The index.yaml file doesn't have `entries`!")?;

    let chart_versions = entries_value.get(chart_name)
        .with_context(|| format!("Couldn't find chart `{}` in index.yaml file!", chart_name))?;

    let chart_versions_seq = chart_versions.as_sequence()
        .with_context(|| format!("The syntax of the chart entries for chart `{}` is incorrect!", chart_name))?;

    let current_version_chart_info = chart_versions_seq.iter().find(filter_chart_versions_info)?;
}

fn filter_chart_versions_info(chart_info: &Value) -> bool {
    return true;
}

fn get_latest_version_values_file(tmp_dir: &TempDir, latest_chart_info: &Value) -> Result<PathBuf> {
    let chart_name = latest_chart_info.get("name").with_context(|| "Couldn't find property `name` in chart info!")?.as_str().unwrap();

    let latest_chart_archive_path = download_chart_archive(&tmp_dir, latest_chart_info)
        .with_context(|| "Couldn't download the latest chart archive!")?;
    let latest_chart_archive_path_str = latest_chart_archive_path.to_str().unwrap();
    let latest_chart_untared_path = untar_archive(&latest_chart_archive_path, &tmp_dir)
        .with_context(|| format!("Failed to untar the chart archive `{}`!", &latest_chart_archive_path_str))?;
    let latest_chart_values_file_path = latest_chart_untared_path.join(format!("{}/values.yaml", chart_name));

    return Ok(latest_chart_values_file_path);
}

fn download_chart_archive(tmp_dir: &TempDir, latest_chart_info: &Value) -> Result<PathBuf> {
    let latest_chart_urls_seq = latest_chart_info.get("urls")
        .with_context(|| "Could not find the `urls` property in the latest chart version!")?
        .as_sequence().unwrap();
    let latest_chart_url_str = latest_chart_urls_seq.first()
        .with_context(|| "Could not retrieve the latest url for chart!")?
        .as_str().unwrap();
    let latest_chart_archive_path = download_file_to_temp(&tmp_dir, latest_chart_url_str)
        .with_context(|| format!("Failed to download chart archive from `{}`!", latest_chart_url_str))?;

    return Ok(latest_chart_archive_path);
}

fn get_latest_chart_info_from_index<'a>(chart_name: &str, index_yaml_content: &'a Value) -> Result<&'a Value> {
    let entries_value = index_yaml_content.get("entries")
        .with_context(|| "The index.yaml file doesn't have `entries`!")?;

    let chart_versions = entries_value.get(chart_name)
        .with_context(|| format!("Couldn't find chart `{}` in index.yaml file!", chart_name))?;

    let chart_versions_seq = chart_versions.as_sequence()
        .with_context(|| format!("The syntax of the chart entries for chart `{}` is incorrect!", chart_name))?;

    let latest_chart_info = chart_versions_seq.first()
        .with_context(|| format!("Could not get the latest chart entry for the chart `{}`!", chart_name))?;

    println!("{:?}", latest_chart_info);

    Ok(latest_chart_info)
}

fn parse_yaml_file(file_path: &PathBuf) -> Result<Value> {
    println!("{:?}", file_path);
    let file_path_str = file_path.to_str().unwrap();
    let file = File::open(file_path)
        .with_context(|| format!("Could not open file `{}`", file_path_str))?;
    let file_reader = BufReader::new(file);
    let file_content: Value = serde_yaml::from_reader(file_reader).with_context(|| "Could not parse yaml file!")?;

    Ok(file_content)
}

fn download_file_to_temp(tmp_dir: &TempDir, target: &str) -> Result<PathBuf> {
    let response = ureq::get(target).call();
    println!("{:?}", response);
    let temp_file_path = tmp_dir.path().join(generate_rand_filename());

    return if response.ok() {
        let mut temp_file = File::create(&temp_file_path)
            .with_context(|| format!("An error occurred while creating a tempfile in folder `{}`", tmp_dir.path().display()))?;

        copy(&mut response.into_reader(), &mut temp_file)?;

        Ok(temp_file_path)
    } else {
        Err(anyhow::anyhow!("Fetching the file failed with `{}`!", &response.status_line()))
    }
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
