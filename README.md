# Helm me up, man! (hmum)
A simple tool to help automate some parts of the work needed to update Kubernetes deployments done with [Helm 3](https://helm.sh/) and/or [helmsman](https://github.com/Praqma/helmsman).

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

TODO

## How to use?

TODO

## Assumptions:

## TODOs:

* Docs
* Tests
* Support for Helm charts (need to implement custom parser for the chart parameter to pass name, version and repo)
* Add more apps in the helmsman examples (maybe from different repos?)
* Add support for multiple valuesFiles in helmsman.config.yaml parsing