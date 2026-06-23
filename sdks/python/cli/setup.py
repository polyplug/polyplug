"""
setup.py — forces a platform-specific (non-pure) wheel.

Subclassing Distribution with `has_ext_modules = lambda self: True` tricks
setuptools/wheel into tagging the wheel with the running platform
(e.g. manylinux2014_x86_64, macosx_11_0_arm64, win_amd64) instead of the
generic py3-none-any tag.  This is the standard technique used by projects
like ruff and uv that ship prebuilt binaries inside Python wheels.

For CI builds where the running host does not match the target platform,
pass --plat-name explicitly to bdist_wheel; see the CI build commands in
README.md.
"""

from setuptools import setup
from setuptools.dist import Distribution


class BinaryDistribution(Distribution):
    def has_ext_modules(self) -> bool:
        return True


setup(distclass=BinaryDistribution)
