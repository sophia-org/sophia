"""Metadata-only admission. Never open a DRM device or change a udev rule."""
import ctypes as C
import os
from pathlib import Path
import re
import stat


def udev_cards(library):
    lib = C.CDLL(library)
    # All libudev ownership stays local; strings are copied before unref.
    signatures = {
        "udev_new": (C.c_void_p, []), "udev_unref": (C.c_void_p, [C.c_void_p]),
        "udev_enumerate_new": (C.c_void_p, [C.c_void_p]),
        "udev_enumerate_unref": (C.c_void_p, [C.c_void_p]),
        "udev_enumerate_add_match_subsystem": (C.c_int, [C.c_void_p, C.c_char_p]),
        "udev_enumerate_scan_devices": (C.c_int, [C.c_void_p]),
        "udev_enumerate_get_list_entry": (C.c_void_p, [C.c_void_p]),
        "udev_list_entry_get_next": (C.c_void_p, [C.c_void_p]),
        "udev_list_entry_get_name": (C.c_char_p, [C.c_void_p]),
        "udev_device_new_from_syspath": (C.c_void_p, [C.c_void_p, C.c_char_p]),
        "udev_device_unref": (C.c_void_p, [C.c_void_p]),
        "udev_device_get_is_initialized": (C.c_int, [C.c_void_p]),
        "udev_device_get_devnode": (C.c_char_p, [C.c_void_p]),
        "udev_device_get_property_value": (C.c_char_p, [C.c_void_p, C.c_char_p]),
    }
    for name, (result, arguments) in signatures.items():
        function = getattr(lib, name)
        function.restype, function.argtypes = result, arguments
    context = lib.udev_new()
    if not context:
        raise ValueError("udev context unavailable")
    enumeration = None
    rows = []
    try:
        enumeration = lib.udev_enumerate_new(context)
        if not enumeration or lib.udev_enumerate_add_match_subsystem(enumeration, b"drm") < 0:
            raise ValueError("udev enumeration unavailable")
        if lib.udev_enumerate_scan_devices(enumeration) < 0:
            raise ValueError("udev scan failed")
        entry = lib.udev_enumerate_get_list_entry(enumeration)
        while entry:
            name = lib.udev_list_entry_get_name(entry)
            if name and re.fullmatch(r"card[0-9]+", Path(os.fsdecode(name)).name):
                device = lib.udev_device_new_from_syspath(context, name)
                if not device:
                    raise ValueError("card disappeared during enumeration")
                try:
                    node = lib.udev_device_get_devnode(device)
                    seat = lib.udev_device_get_property_value(device, b"ID_SEAT")
                    rows.append({"sysfs": os.fsdecode(name),
                                 "initialized": lib.udev_device_get_is_initialized(device) > 0,
                                 "node": os.fsdecode(node) if node else None,
                                 "seat": os.fsdecode(seat) if seat else "seat0"})
                finally:
                    lib.udev_device_unref(device)
            entry = lib.udev_list_entry_get_next(entry)
    finally:
        if enumeration:
            lib.udev_enumerate_unref(enumeration)
        lib.udev_unref(context)
    return rows


def device_record(node, entry):
    info = node.stat()
    major, minor = map(int, (entry / "dev").read_text().strip().split(":"))
    if not stat.S_ISCHR(info.st_mode) or info.st_rdev != os.makedev(major, minor):
        raise ValueError("DRM node and sysfs identity disagree")
    return {"node": str(node), "major": major, "minor": minor,
            "filesystem": info.st_dev, "inode": info.st_ino}


def select(rows, seat, pci):
    # Match Sophia's global uninitialized-card refusal. No provisional seat is
    # authoritative until initialization finishes, even for a foreign card.
    if any(not row["initialized"] for row in rows):
        raise ValueError("uninitialized DRM card")
    local = [row for row in rows if row["seat"] == seat]
    if len(local) != 1:
        raise ValueError("development seat must admit exactly one primary DRM card")
    row = local[0]
    entry = Path(row["sysfs"]).resolve(strict=True)
    node = Path("/dev/dri") / entry.name
    if row["node"] != str(node) or (entry / "device").resolve(strict=True).name != pci:
        raise ValueError("development card does not match the fixed PCI identity")
    render = [item for item in entry.parent.iterdir()
              if re.fullmatch(r"renderD[0-9]+", item.name)]
    if len(render) != 1:
        raise ValueError("expected one render sibling in the same DRM parent")
    sibling = render[0].resolve(strict=True)
    if sibling.parent != entry.parent or (sibling / "device").resolve(strict=True).name != pci:
        raise ValueError("render sibling identity mismatch")
    return {"pci": pci, "seat": seat, "sysfs": str(entry),
            "nodes": [device_record(node, entry),
                      device_record(Path("/dev/dri") / sibling.name, sibling)]}


def discover(config):
    return select(udev_cards(config["tools"]["libudev"]["path"]), config["seat"], config["pci"])
