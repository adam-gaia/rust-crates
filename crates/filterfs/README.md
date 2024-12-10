# filterfs

filterfs is a userspace "filesystem" that allows hiding files with glob patterns.

## Disclaimer

filterfs is a work-in-progress. It's useable for my workflows (checkout \[anyignore\](todo: link)!), but still rough around the edges.
While I don't think dataloss will happen, I'm not making any promises.

## Usage

```
Usage: filterfs [OPTIONS] <MOUNTPOINT> <SOURCE>

Arguments:
  <MOUNTPOINT>  Act as a client, and mount FUSE at given path
  <SOURCE>      Directory to filters files from

Options:
  -e, --exclude <EXCLUDE>  Paths/glob patterns to exclude
      --auto-unmount       Automatically unmount on process exit
      --allow-root         Allow root user to access filesystem
      --hidden             Exclude all hidden files
  -h, --help               Print help
```

### Example

```console
$ mountpoint='/tmp/mountpoint'
$ source="${PWD}/testing"

$ mkdir "${mountpoint}"
$ mkdir "${source}"

$ touch "${source}/foo" # This file will be hidden at the file system level
$ touch "${source}/bar" # This file will still be accessible

$ filterfs "${mountpoint}" "${source}" --exclude="foo" & # Run in the background. Process will stop when mountpoint is unmounted

$ ls ${source} # Shows both files
bar foo

$ ls ${mountpoint} # Shows only 'bar'
bar

$ umount "${mountpoint}" # Unmounts file system and stops the 'filterfs' process
```

## anyignore

filterfs was created to support \[anyignore\](TODO: add link) (but is still a useful tool on its own)
