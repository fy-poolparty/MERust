# UE Viewer (umodel)

`umodel_64.exe` and `SDL2_64.dll` are the unmodified Windows 64-bit build of
[UE Viewer](https://www.gildor.org/en/projects/umodel) by Konstantin Nosov (Gildor), source at
[github.com/gildor2/UEViewer](https://github.com/gildor2/UEViewer). UE Viewer is under the MIT
license (see `LICENSE.txt`); SDL2 is under the zlib license.

MERust runs it on your own Mirror's Edge install to export the character, gun and UI meshes into
`cache/` (git-ignored). No game content is stored here.

To use another build, set the `UMODEL` environment variable to its `umodel_64.exe`.
