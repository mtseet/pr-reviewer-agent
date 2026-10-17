# pr-reviewer-agent
pr-reviewer-agent - An AI agent for reviewing prs

## Description
An AI Agent that reviews prs

### Features
- gitea support
- github support

## Screenshot
<div style="display: flex; gap: 10px;">
<img src="screenshots/Screenshot_1.png" alt="Alt text" width="250">

<img src="screenshots/Screenshot_2.png" alt="Alt text" width="250">
</div>

<div style="display: flex; gap: 10px;">
<img src="screenshots/Screenshot_3.png" alt="Alt text" width="250">

<img src="screenshots/Screenshot_4.png" alt="Alt text" width="250">
</div>

<div style="display: flex; gap: 10px;">
<img src="screenshots/Screenshot_5.png" alt="Alt text" width="250" height="500">

<img src="screenshots/Screenshot_6.png" alt="Alt text" width="250" height="500">
</div>

## Binary

## Installationn and Build Instractions

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

cargo build
cargo run
```

## Usage Instructions
```
pr-reviewer-agent --help
AI-based PR reviewer for Gitea/GitHub

Usage: pr-reviewer-agent --repotype <REPOTYPE> --repo <REPO> --prid <PRID>

Options:
      --repotype <REPOTYPE>  Repository type (gitea, github)
      --repo <REPO>          Repository name (or owner/repo)
      --prid <PRID>          Pull request ID
  -h, --help                 Print help
  -V, --version              Print version
```

## Support
You can buy me a coffee if you like this software

<a href="https://www.buymeacoffee.com/mtseet" target="_blank"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me A Coffee" style="height: 60px !important;width: 217px !important;" ></a>