# School measurement data dictionary

The template header is required. The last four fields may be blank. Each row is one timed observation.

| Column | Format |
|---|---|
| measurement_id | Unique anonymous label |
| timestamp | ISO 8601 date and time with a UTC offset |
| location_id | Anonymous repeatable receiver label |
| location_type | Space type such as classroom, library, laboratory or outdoor |
| background_level | Finite numeric reading from device |
| unit | LAeq_dBA, LZeq_dB_SPL, or app_estimate_dBA |
| duration_s | Positive measurement duration in seconds |
| device_model | Device or phone/app combination |
| calibration_status | field_checked, not_checked, or unknown |
| temperature_c | Optional, degrees Celsius |
| humidity_pct | Optional, 0 to 100 |
| weather | Optional anonymous condition such as dry or windy |
| notes | Optional brief conditions, with no identifying information |

The summary keeps each location, unit, duration, device model and calibration status separate. Meter equivalents use an energy mean across equal-duration samples. App estimates receive a count and range. Inspect timestamp and notes before interpreting temporal differences. If comparing only like-for-like conditions, filter the input to the relevant period first.
