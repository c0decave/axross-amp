"""axross's save_state as its PlayerPanel does it, with axross's own code:
its in-memory state (stdin, JSON) merged with the state file by
merge_shared_state against the playlist it last read (its base: the
file's playlist when this script is given BASE_FILE), then written by
write_state.  python axross_save.py AXROSS_DIR PATH BASE_FILE"""
import importlib.util
import json
import sys
from pathlib import Path

axross, path, base_file = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
sys.path.insert(0, str(axross))
spec = importlib.util.spec_from_file_location("mp", axross / "resources/extensions/media_player.py")
mp = importlib.util.module_from_spec(spec)
sys.modules["mp"] = mp
spec.loader.exec_module(mp)
own = json.load(sys.stdin)
own.setdefault("version", mp.STATE_VERSION)
base = mp.playlist_base(json.loads(Path(base_file).read_text()))
stored, _ = mp.read_state(path)
merged = mp.merge_shared_state(own, stored, base)
mp.write_state(path, merged, plain=mp.plain_state(merged, own))
print(json.dumps([t["path"] for t in mp.read_state(path)[0]["tracks"]]))
