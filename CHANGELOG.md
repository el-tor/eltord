# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [v0.0.2] - 2025-10-26
Release 0.0.2 - Connectivity and Bandwidth improvements

## [v0.0.1] - 2025-07-26
Initial release of eltord 0.0.1
### Added
- ✅ **Process Cleanup**: Added proper cleanup of spawned Tor processes on app termination
- ✅ **Cross-Platform Support**: Full Windows support with `taskkill` process management  
- ✅ **Signal Handling**: Graceful shutdown on Ctrl-C and other termination signals
- 🔧 **Process Isolation**: Maintains crash protection while ensuring clean shutdown

### Fixed
- 🐛 Fixed orphaned Tor processes when main application is killed
- 🐛 Fixed "MultipleHandlers" panic from signal handler registration
- 🐛 Improved Windows subprocess spawning and tracking

### Changed
- 🔧 **Code Organization**: Refactored signal handling to prevent duplicate handler registration
- 🔧 **Error Handling**: Better error messages and logging for process management
