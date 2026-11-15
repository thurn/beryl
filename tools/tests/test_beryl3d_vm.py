"""`beryl3d-bench`, Beryl 3D's count machine."""

from beryl_tools import bench
from beryl_tools.beryl3d import vm


def test_commands_go_to_the_named_machine_and_the_engine_keeps_its_own() -> None:
    assert vm.NAME in bench.vm_command("true", vm.NAME)
    assert bench.VM in bench.vm_command("true")
    assert vm.NAME not in bench.vm_command("true")


def test_provisioning_adds_the_vulkan_drivers_to_the_engine_machine_s() -> None:
    script = vm.provision_script("1.0.0", "0.20.0")
    assert script.startswith(bench.provision_script("1.0.0", "0.20.0"))
    assert "mesa-vulkan-drivers" in script


def test_the_runner_matches_a_locked_gungraun() -> None:
    assert vm.runner_version() is not None
