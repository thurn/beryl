"""The exit code for a command that could not run on this host."""

# The exit code with which a step says it could not run on this machine, such as a missing
# simulator, as distinct from a failure. Ignite's adapters report it as incomplete.
CANNOT_RUN = 125
