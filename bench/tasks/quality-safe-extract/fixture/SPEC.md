# Safe extraction

Implement `extract(entries, root) -> list[str]`. Entries are a list of
`(name, data)` tuples for regular files; name is a string and data is bytes.
Root is a pathlib.Path or string naming an existing directory. Return entry
names in input order. The caller owns a stable filesystem: concurrent changes
and rollback after I/O errors are outside this API's scope.

Before creating ANY file or directory, validate the ENTIRE batch:

- Reject malformed entries, non-string names, and non-bytes data with ValueError.
- Names must be nonempty relative POSIX paths. Reject leading '/', backslashes,
  colons (including drive letters), NUL, and any empty, '.' or '..' component.
  Do not sanitize, normalize, or decode names. Literal percent signs are legal.
- Reject duplicate names and file/directory collisions within the batch, in
  either order (for example 'a' and 'a/b').
- Root must be an existing real directory, not a symlink. No existing component
  below root may be a symlink, including dangling symlinks and the target.
  Every existing ancestor below root must be a directory; each target must
  be absent. Never overwrite existing files or follow links, even links to
  locations inside root. Reject invalid roots and all conflicts with ValueError.
- Validation failure must leave root and outside files/directories unchanged.
  Empty batches succeed with [] after root validation. Valid batches create
  missing parent directories and write exact bytes, including empty files.

Only regular-file entries are supported; no archive or third-party library is
needed. Unexpected filesystem I/O errors must remain visible.
