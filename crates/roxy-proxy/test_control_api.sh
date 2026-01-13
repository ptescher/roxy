#!/bin/bash
# Test script for Roxy Control API
# Assumes the proxy is running with control API on port 8889

set -e

CONTROL_URL="http://127.0.0.1:8889"

echo "=== Roxy Control API Test Suite ==="
echo ""

# Test 1: Health check
echo "Test 1: Health Check"
echo "GET ${CONTROL_URL}/health"
HEALTH_RESPONSE=$(curl -s "${CONTROL_URL}/health")
echo "Response: ${HEALTH_RESPONSE}"
if [ "$HEALTH_RESPONSE" = "OK" ]; then
    echo "✓ Health check passed"
else
    echo "✗ Health check failed"
    exit 1
fi
echo ""

# Test 2: Get initial status
echo "Test 2: Get Status"
echo "GET ${CONTROL_URL}/status"
STATUS_RESPONSE=$(curl -s "${CONTROL_URL}/status")
echo "Response: ${STATUS_RESPONSE}"
echo "✓ Status retrieved"
echo ""

# Test 3: Enable auto port-forward
echo "Test 3: Enable Auto Port-Forward"
echo "POST ${CONTROL_URL}/control"
echo 'Body: {"command":"set_auto_port_forward","enabled":true}'
ENABLE_RESPONSE=$(curl -s -X POST "${CONTROL_URL}/control" \
  -H "Content-Type: application/json" \
  -d '{"command":"set_auto_port_forward","enabled":true}')
echo "Response: ${ENABLE_RESPONSE}"

# Check if response contains success:true
if echo "$ENABLE_RESPONSE" | grep -q '"success":true'; then
    echo "✓ Auto port-forward enabled"
else
    echo "✗ Failed to enable auto port-forward"
    exit 1
fi
echo ""

# Test 4: Verify status changed
echo "Test 4: Verify Status Changed to Enabled"
echo "GET ${CONTROL_URL}/status"
STATUS_RESPONSE=$(curl -s "${CONTROL_URL}/status")
echo "Response: ${STATUS_RESPONSE}"

if echo "$STATUS_RESPONSE" | grep -q '"auto_port_forward_enabled":true'; then
    echo "✓ Status shows enabled"
else
    echo "✗ Status does not show enabled"
    exit 1
fi
echo ""

# Test 5: Disable auto port-forward
echo "Test 5: Disable Auto Port-Forward"
echo "POST ${CONTROL_URL}/control"
echo 'Body: {"command":"set_auto_port_forward","enabled":false}'
DISABLE_RESPONSE=$(curl -s -X POST "${CONTROL_URL}/control" \
  -H "Content-Type: application/json" \
  -d '{"command":"set_auto_port_forward","enabled":false}')
echo "Response: ${DISABLE_RESPONSE}"

if echo "$DISABLE_RESPONSE" | grep -q '"success":true'; then
    echo "✓ Auto port-forward disabled"
else
    echo "✗ Failed to disable auto port-forward"
    exit 1
fi
echo ""

# Test 6: Verify status changed to disabled
echo "Test 6: Verify Status Changed to Disabled"
echo "GET ${CONTROL_URL}/status"
STATUS_RESPONSE=$(curl -s "${CONTROL_URL}/status")
echo "Response: ${STATUS_RESPONSE}"

if echo "$STATUS_RESPONSE" | grep -q '"auto_port_forward_enabled":false'; then
    echo "✓ Status shows disabled"
else
    echo "✗ Status does not show disabled"
    exit 1
fi
echo ""

# Test 7: Get status via POST command
echo "Test 7: Get Status via POST Command"
echo "POST ${CONTROL_URL}/control"
echo 'Body: {"command":"get_status"}'
STATUS_CMD_RESPONSE=$(curl -s -X POST "${CONTROL_URL}/control" \
  -H "Content-Type: application/json" \
  -d '{"command":"get_status"}')
echo "Response: ${STATUS_CMD_RESPONSE}"

if echo "$STATUS_CMD_RESPONSE" | grep -q '"success":true'; then
    echo "✓ Get status command works"
else
    echo "✗ Get status command failed"
    exit 1
fi
echo ""

# Test 8: Invalid command
echo "Test 8: Invalid Command (should fail gracefully)"
echo "POST ${CONTROL_URL}/control"
echo 'Body: {"command":"invalid_command"}'
INVALID_RESPONSE=$(curl -s -X POST "${CONTROL_URL}/control" \
  -H "Content-Type: application/json" \
  -d '{"command":"invalid_command"}')
echo "Response: ${INVALID_RESPONSE}"
echo "✓ Invalid command handled"
echo ""

# Re-enable for cleanup
echo "Cleanup: Re-enabling auto port-forward"
curl -s -X POST "${CONTROL_URL}/control" \
  -H "Content-Type: application/json" \
  -d '{"command":"set_auto_port_forward","enabled":true}' > /dev/null
echo ""

echo "=== All Tests Passed! ==="
