# acpr

`acpr` runs agents from the [ACP registry](https://agentclientprotocol.com/get-started/registry).

## Usage

Run an agent:
```bash
acpr <agent-name>
```

List available agents:
```bash
acpr --list
```

## To install it

* `cargo binstall acpr` or `cargo install acpr`, if you have the Rust toolchain installed

## How it works

`acpr` fetches the ACP registry and runs the specified agent using the appropriate method:

- **npm packages**: `npx -y package@latest`
- **Python packages**: `uvx package@latest` 
- **Binaries**: Downloads, extracts, and executes platform-specific binaries

Downloaded files are cached locally. The registry is refreshed every 3 hours.

## Options

- `--list` - Show available agents
- `--force <type>` - Force refresh (`all`, `registry`, or `binary`)
- `--cache-dir <dir>` - Custom cache directory
- `--registry <file>` - Use custom registry file
- `--debug` - Show debug output

## Examples

```bash
# Run cline agent
acpr cline

# List all available agents
acpr --list

# Force refresh and run
acpr --force all goose

# Use custom cache directory
acpr --cache-dir ./cache cursor
```
