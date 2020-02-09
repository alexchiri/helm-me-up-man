use std::path::PathBuf;
use structopt::StructOpt;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::BufReader;
use failure::ResultExt;
use exitfailure::ExitFailure;
use url::Url;
use serde_yaml::{Value, Mapping};

#[derive(Debug)]
struct Repo {
    name: String,
    url: Url,
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
    dsf_path: PathBuf,
    apps: Vec<App>,
}

#[derive(Debug, StructOpt)]
#[structopt(name = "hmum", about = "A tool to help update Helm charts and/or helmsman DSFs")]
struct Args {
    #[structopt(short = "f", long, parse(from_os_str))]
    helmsmanconfig: Option<Vec<PathBuf>>,
}

fn main() -> Result<(), ExitFailure> {
    let args = Args::from_args();
    println!("{:?}", args);

    let helmsman_file_paths = &args.helmsmanconfig.expect("You should provide at least one helmsman config file path!");

    let mut helm_repos: Vec<Repo> = Vec::new();

    // Process all the helmsman DSFs and repos
    let mut helmsman_confs = Vec::new();

    for helmsman_file_path in helmsman_file_paths {
        println!("{:?}", helmsman_file_path);
        let helmsman_file_path_str = helmsman_file_path.to_str().unwrap();

        let helmsman_file = File::open(helmsman_file_path).with_context(|_| format!("Could not open file `{}`", helmsman_file_path_str))?;
        let helmsman_file_reader = BufReader::new(helmsman_file);
        let helmsman_config: Value = serde_yaml::from_reader(helmsman_file_reader).with_context(|_| format!("Could not parse helmsman config file `{}`", helmsman_file_path_str))?;
        println!("{:?}", helmsman_config);

        // Process all the repos
        let helm_repos_value = helmsman_config.get("helmRepos").expect(&format!("The helmsman DSF `{}` doesn't define `helmRepos`!", helmsman_file_path_str));
        let helm_repos_conf = helm_repos_value.as_mapping().expect(&format!("The `helmRepos` syntax in helmsman DSF `{}` is incorrect!", helmsman_file_path_str));

        for helm_repo_conf in helm_repos_conf.iter() {
            let repo_name_str: String = String::from(helm_repo_conf.0.as_str().expect(&format!("Helm repo name is not a proper String in `{}`", helmsman_file_path_str)));
            let repo_url_str = helm_repo_conf.1.as_str().expect(&format!("Helm repo URL is not a proper String in `{}`", helmsman_file_path_str));
            let repo_url = Url::parse(repo_url_str).with_context(|_| format!("Could not parse URL in helmsman DSF `{}`", repo_url_str))?;

            helm_repos.push(Repo {
                name: repo_name_str,
                url: repo_url,
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
                values_file_path: PathBuf::from(app_values_file_str)
            });

        }

        println!("{:?}", apps);

        helmsman_confs.push(Helmsman {
            dsf_path: PathBuf::from(helmsman_file_path_str),
            apps
        });

    }

    println!("{:?}", helmsman_confs);

    Ok(())
}
