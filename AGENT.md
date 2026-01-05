# AGENTS instructions

The goal of this project is to combine a subset of the features of Wireshark, Proxyman, and Kubectl, to enable developers to work in a hybrid local/cloud environment. They should be able to see and debug network connections, forward subsets of api gateway traffic to local instances (not pushed via CI/CD, local debugger, etc), and get relavent insights to help them understand complex microservice meshes.

This repo exists as an open source tool for developers. We expect it to be easy to maintain, and expand, so use rustdoc etc when it makes sense. It should have some tests, but keep it to high level user flows.

When adding new features, think about the correct domain abstractions and implement libraries as necessary.

Write documentation targeting software engineers. Keep it short and to the point. We don't need to list out features, examples, or architecture unless strictly necessary.
