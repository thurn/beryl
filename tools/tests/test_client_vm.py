"""`beryl3d-bench`, The client's count machine."""

from beryl_tools import bench, system
from beryl_tools.client import vm


def test_commands_go_to_the_named_machine_and_the_engine_keeps_its_own() -> None:
    assert vm.NAME in bench.vm_command("true", vm.NAME)
    assert bench.VM in bench.vm_command("true")
    assert vm.NAME not in bench.vm_command("true")


def test_provisioning_adds_the_vulkan_drivers_to_the_engine_machine_s() -> None:
    script = vm.provision_script("1.0.0")
    assert script.startswith(bench.provision_script("1.0.0", None))
    assert "mesa-vulkan-drivers" in script
    # the client counts with Callgrind alone, so the machine needs no Gungraun runner.
    assert "gungraun" not in script
    assert "gungraun-runner" in bench.provision_script("1.0.0", "0.20.0")


def test_the_machine_s_packages_change_only_when_provisioning_installs_them() -> None:
    assert "systemctl disable --now unattended-upgrades" in vm.provision_script("1.0.0")


def test_provisioning_runs_from_the_machine_s_root() -> None:
    # The sweep's worktree is outside the main checkout, the only directory the machine
    # mounts, so a command run from the checkout's own path fails to start there.
    command = bench.vm_command("true", vm.NAME, "/")
    directory_flag = "--cd" if system.WINDOWS else "--workdir"
    assert command[command.index(directory_flag) + 1] == "/"
