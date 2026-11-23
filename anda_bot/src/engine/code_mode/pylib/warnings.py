"""Anda's stand-in for warnings.

The frozen modules import warnings only to report deprecations, such as a
positional count passed to re.sub() or datetime.utcnow(). Code Mode scripts
have no stderr, so the notices are dropped. The upstream module needs the
native _contextvars, which the worker does not include.
"""


def warn(*args, **kwargs):
    """Drop the warning."""


def _deprecated(*args, **kwargs):
    """Drop the deprecation notice."""
