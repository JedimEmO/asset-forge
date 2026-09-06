//! Project-specific agent instructions and local MCP launch configuration.

use std::io::Write;
use std::path::Path;

use crate::{LibraryError, Project, Result};

/// Write missing agent files without changing existing instructions or configuration.
///
/// The standalone `.forge/mcp.json` can be merged into any MCP client; a new
/// project's `.mcp.json` receives the same configuration. Paths are absolute
/// and arguments are JSON arrays, so launching requires no shell or fixed cwd.
///
/// # Errors
/// Returns an error if paths cannot be resolved or files cannot be written.
/// Already existing files are preserved and named in the returned report.
pub fn write(project: &Project, executable: &Path, toolkit: Option<&Path>) -> Result<Vec<String>> {
    let root = project
        .root
        .canonicalize()
        .map_err(|e| LibraryError::io(&project.root, e))?;
    let executable = executable
        .canonicalize()
        .map_err(|e| LibraryError::io(executable, e))?;
    let toolkit = toolkit
        .map(|path| path.canonicalize().map_err(|e| LibraryError::io(path, e)))
        .transpose()?;
    let dir = root.join(".forge");
    if dir
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(LibraryError::rejected(
            ".forge must be a project-owned directory, not a symlink",
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| LibraryError::io(&dir, e))?;
    let mut env = serde_json::Map::new();
    if let Some(toolkit) = &toolkit {
        env.insert(String::from("FORGE_HOME"), serde_json::json!(toolkit));
    }
    let config = serde_json::json!({"mcpServers": {"asset-forge": {
        "command": executable, "args": ["mcp", "--project", root], "env": env
    }}});
    let config = format!("{config:#}\n");
    let guide = format!(
        "# Asset Forge in this game\n\nToolkit executable: {}\n\nProject root: {}\n\n\
         Start the local MCP server using `.forge/mcp.json`. Read its MCP resource `forge://guides/v1/workflow`. Its explicit project argument\n\
         keeps this game's library separate from other games and the toolkit.\n\
         Merge the `asset-forge` server entry into your client's configuration if it already\n\
         has one. Existing instruction and configuration files are never overwritten.\n\n\
         For CLI calls, use the executable above with `--project` and this project root,\n\
         followed by the command. Pass arguments separately; quote paths when using a shell.\n\
         Read the full workflow guide with `forge guide` (using the executable above);\n\
         it is the same embedded guide as the MCP resource and needs no project or backend.\n\
         Keep the configured `FORGE_HOME` environment for generation.\n\n\
         Start with `doctor --quick`. Read MCP `licences` or CLI `setup --dry-run` before setup; accept required terms\n\
         by name only with the user's authorization. Model environments are shared machine\n\
         resources. Each game owns `forge.toml`, `assets-src`, `assets`, and `out`.\n\n\
         Read a tool's schema or a command's `--help` before supplying generation parameters.\n\
         Import externally created references with `ref import` / `import_reference`.\n\
         Generate one candidate, wait for its terminal job status, then inspect it.\n\
         Review geometry from all views, clips on the real body, and audio against the brief.\n\
         Reject unsuitable candidates and revise their source or parameters. Never repair\n\
         derived meshes, baked clips, or provenance records by hand.\n\n\
         Promote only reviewed candidates. Finish with `verify`, `audit`, and\n\
         `manifest --check` through the CLI, or MCP `verify`, `audit`, `manifest_check`.\n\
         Export a body and its clips with `bundle` / `export_bundle`. File integrity checks\n\
         do not establish visual or audio quality. Fake generation only tests the workflow.\n\n\
         If this installation or project moves, regenerate launch files with `agent-config`\n\
         after removing only the generated files you want replaced. Preserve custom settings.\n",
        executable.display(),
        root.display()
    );
    let mut lines = Vec::new();
    for (path, text) in [
        (dir.join("AGENT.md"), guide.as_str()),
        (dir.join("mcp.json"), config.as_str()),
        (root.join(".mcp.json"), config.as_str()),
        (
            root.join("AGENTS.md"),
            "# Asset production\n\nRead `.forge/AGENT.md` before using Asset Forge in this project.\n",
        ),
    ] {
        if write_missing(&path, text.as_bytes())? {
            lines.push(format!("wrote {}", path.display()));
        } else {
            lines.push(format!(
                "preserved {} — read .forge/AGENT.md and merge .forge/mcp.json as needed",
                path.display()
            ));
        }
    }
    Ok(lines)
}

// Publish complete files without replacing another writer's configuration.
fn write_missing(path: &Path, bytes: &[u8]) -> Result<bool> {
    if path.symlink_metadata().is_ok() {
        return Ok(false);
    }
    let temporary = crate::temp_sibling(path);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| LibraryError::io(&temporary, e))?;
        file.write_all(bytes)
            .map_err(|e| LibraryError::io(&temporary, e))?;
        file.sync_all()
            .map_err(|e| LibraryError::io(&temporary, e))?;
        match std::fs::hard_link(&temporary, path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(LibraryError::io(path, e)),
        }
    })();
    let _ = std::fs::remove_file(&temporary);
    result
}
