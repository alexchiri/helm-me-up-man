# Helm me up, man! (hmum)
A simple tool to help automate some parts of the work needed to update Kubernetes deployments done with 
 [helmsman](https://github.com/Praqma/helmsman).

## Why would this be useful?

To keep your Helm deployed services up-to-date, you generally need to do the following manual steps for each chart:

1. Check if there is a newer version of the chart
2. Check for changes in the `values.yaml` file in the new chart version and merge them with your customised values file
3. Check release notes for eventual changes you might need to do in configuration to have the new version work as desired 
4. Deploy the new version to a test cluster 
5. Run tests
6. Deploy to production cluster

It would be great to automate this process, but sometimes this (especially step 3) is quite difficult to do. 
Still we can automate some parts of it, while still requiring human input.

The purpose of `hmum` is to automate step 1 and 2, which could be quite consuming, especially when you have quite a few third-party chart files.

## How does it work?

`hmum` is intended to be used inside a pipeline that runs periodically, although there's nothing preventing you to use it as is on your computer. 

It can be provided with one or more helmsman DSFs or one or more Helm chart information sets, or a mix of those, and it will 
1. try to figure out if there are new versions of those charts
2. merge the new values files with the ones provided to it
3. update helmsman DSFs with the new chart version
4. (optional) commit these using git to a new or existing branch. In this case, it will include in the commit message some information about the changes:
    
    1. the version of the new chart version
    2. the version of the app included in the new chart version
    3. if there were any conflicts during the merge.  
    
Ideally, a pipeline would run `hmum` against a repo where the information about the Helm/helmsman releases is kept and create a pull request that is reviewed by developers. Once the result is satisfactory, the PR would be merged and another pipeline would trigger and deploy everything to a test cluster. 

## How to install?

`hmum` requires [`git merge-file`](https://git-scm.com/docs/git-merge-file) (part of the general installation of `git` on all platforms) to be already installed as it uses it merge the values files.

Download binary for your OS from the [releases page](https://github.com/alexchiri/helm-me-up-man/releases), unpack and run the binary. 

## How to use?
Simply pass as many helmsman DSFs using the `-f` option of the CLI.

For example, to update the values files in the examples, run `hmum -f examples/infra.helmsman.config.yaml -f examples/monitoring.helmsman.config.yaml`. If all goes well, the values files will have some changes. The `fluentd` file will also have merge conflicts. 

## Assumptions:
* All helm repos used in a helmsman DSF are defined in the `helmRepos` property. This property is optional according to the [helmsman DSF specification](https://github.com/Praqma/helmsman/blob/master/docs/desired_state_specification.md#helm-repos), but unless the helm repos are specified in the helmsman DSF, then `hmum` cannot know what URL should a repo have.
* If you use the `valuesFiles` property to provide a list of values files, the first is the one that will be used for the merge.
* If there are multiple apps with exactly the same version that needs to be updated in a helmsman DSF, then the version will have to be updated manually. Currently using regex to update versions in the helmsman DSF. Since I couldn't come up with a regex to uniquely identify a version for an app, `hmum` doesn't try to update the version if there are multiple matches to the version regex. This is to keep the rest of the file exactly the same. Will offer a flag in the future to allow parsing/serializing of the helmsman DSF, which allows me to modify just the right version, but it also removes comments of new lines and might make slight changes to the file.
* All values files are using Unix (LF) line endings. If you run `hmum` and see that the resulting values file is one big merge conflict, it's most likely that the values file was using CRLF line endings.

## TODOs:

* Take into account the different line endings
* Support for flag to commit to branch - including to add information about the update in the commit message
* Docs
* Tests
* Support for Helm charts (need to implement custom parser for the chart parameter to pass name, version and repo)
* Support async in how it downloads files
* Parse the same index.yaml file only once