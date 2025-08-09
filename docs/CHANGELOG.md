# Changelog

## Version 2.4.0
- add `--extra-objects` for asset bundles, allowing you to load arbitrary objects by type and name

## Version 2.3.0
- automatically determine unity version from game files, and generate matching asset bundle
- add experimental `--mode asset-shallow` output mode. Instead of copying objects into the assetbundle, generate
references to the original `levelXX` files
- fix bundling of external dependencies in PyPI package

## Version 2.2.0
- add `--mode asset` output mode for random access assetbundles
- support game files as packed `data.unity3d` file in asset bundles
- automatically publish a nuget package with bindings to this library
- performance: parallelize more of the processing
- performance: remove repeated parsing of the same scene file


## Version 2.1.0
- add dynamic completions for the terminal
- support game files as packed `data.unity3d` file
- various bug fixes

## Version 2.0.0
- rust rewrite of the original python tool, for huge performance and memory usage improvements
