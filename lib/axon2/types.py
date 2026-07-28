# Type aliases used across the package. `builtins` is re-exported here because
# the compiled extensions import it via `from axon2.types import builtins`.
# On Python 3 the ``unicode``/``long`` names of Python 2 collapse onto
# ``str``/``int``; the aliases are kept so callers need not change.
import builtins
import datetime
from decimal import Decimal

unicode_type = builtins.str
str_type = builtins.str
int_type = builtins.int
long_type = builtins.int
float_type = builtins.float
bool_type = builtins.bool
bytes_type = builtins.bytes
bytearray_type = builtins.bytearray
date_type = datetime.date
time_type = datetime.time
datetime_type = datetime.datetime
decimal_type = Decimal
none_type = type(None)