# Field verification of map rankings

## Freeze the comparison before recording

Export the project and keep its original `field_plan.json` and `sensitivity.json`. The plan proposes up to three contrasts using a consistently low candidate and a higher-scoring, consistently unselected candidate. It uses the same named receiver zones for each member of a pair. Preserve the plan, including its predicted scores, before seeing any readings. If a point is inaccessible or inappropriate for field work, revise the map and freeze a new plan before collecting the comparison data. The included Chungbuk Science High School example has fictional zone placements and is a software demonstration.

## Collect comparable measurements

Use an independently available, permitted reference sound source and the same reference signal, device placement, orientation and output setting for both candidate positions. This workflow does not require school drone operations. A stationary reference-source pilot checks spatial ordering under the chosen playback conditions. Evidence about UAV noise specifically requires independent observations of the equivalent UAV source under comparable conditions. Record ordinary school background conditions separately if the source-on study is unavailable.

1. Assign a distinct session identifier to each independent visit or acquisition period. Record conditions and the order in a separate anonymous log. Alternate the order of the lower and higher candidate within pairs across visits to reduce time-of-day and drift effects.
2. Use the same field-checked sound-level meter throughout a pair. Put it at the center of each exported receiver zone, using the same height, orientation and settings for the two positions. The app averages nine simulated sample points per zone. Center-only measurements test a simpler ranking at the zone centers.
3. At each receiver, record 60-second A-weighted equivalent sound levels with the source on and with it off under comparable conditions. Use anonymous site, receiver, session and instrument identifiers. Do not collect speech recordings or personal identifiers.
4. Fill the CSV once for each pair, role and receiver in a session. Use the pair identifier from the plan such as `P01`. Use `lower` or `higher` for the frozen model role. Use `R01`, `R02` and so on in the plan's receiver order. Enter `yes` for `calibration_checked` only when the meter was field checked. The same `reference_id` is required across the study.

## Analyze the readings

```bash
python scripts/validate_field_rankings.py path/to/field_plan.json path/to/readings.csv data/school/field_validation.json
```

A pair is eligible when all receivers have both roles, the device matches, and every source-on reading exceeds its matched background by at least 6 dB. The margin is a prespecified screening choice for this protocol. The evaluator subtracts background in linear energy units, applies the frozen receiver weights, and compares the highest weighted receiver energy for `higher` and `lower`. A positive difference agrees with the map. The output records exclusions and summarizes the median paired difference per independent session, the count of directional agreements and an exploratory one-sided sign test. Exact-zero sessions do not contribute to the sign test. A small number of sessions limits any statistical conclusion. The empty template intentionally returns `awaiting_measurements`.
