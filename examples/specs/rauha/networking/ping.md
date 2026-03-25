# VM-to-VM Ping

Verify that zones can reach each other over the internal network.

## Prerequisites

- `rauhad` is running on the host
- No pre-existing zones named `vm1`, `vm2`, or `vm3`

## Create three zones

- `rauha zone create --name vm1` succeeds (exit code 0)
- `rauha zone create --name vm2` succeeds (exit code 0)
- `rauha zone create --name vm3` succeeds (exit code 0)
- Each zone gets a unique IP on the `10.89.0.0/16` subnet

## Verify zones are running

- `rauha zone list` shows all three zones in a running state
- Each zone has a distinct IP address

## Ping from vm1 to vm3

- Execute a ping from inside vm1 to vm3's IP address
- The ping succeeds (exit code 0, replies received)
- Round-trip time is under 50ms (local bridge network)

## Cleanup

- `rauha zone delete --name vm1` succeeds
- `rauha zone delete --name vm2` succeeds
- `rauha zone delete --name vm3` succeeds
- `rauha zone list` no longer shows any of the three zones
