#!/bin/bash
# Initializes Keycloak with a permanent admin account.
# Used as the entrypoint in docker-compose.test-idp.yml.

set -e

# Start Keycloak in the background
/opt/keycloak/bin/kc.sh start-dev --http-port=9090 &
KC_PID=$!

# Wait for Keycloak to be ready (use bash TCP — curl may not be available)
echo "Waiting for Keycloak to start..."
until (exec 3<>/dev/tcp/127.0.0.1/9000 && echo -e 'GET /health/ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n' >&3 && grep -q 'UP' <&3) 2>/dev/null; do
  sleep 2
done
echo "Keycloak is ready."

KCADM="/opt/keycloak/bin/kcadm.sh"

# Authenticate — try bootstrap admin first, fall back to permanent admin (restart case)
AUTHENTICATED=false
if $KCADM config credentials \
  --server http://localhost:9090 \
  --realm master \
  --user "$KC_BOOTSTRAP_ADMIN_USERNAME" \
  --password "$KC_BOOTSTRAP_ADMIN_PASSWORD" 2>/dev/null; then
  echo "Authenticated as bootstrap admin."
  AUTHENTICATED=true
elif $KCADM config credentials \
  --server http://localhost:9090 \
  --realm master \
  --user admin \
  --password admin 2>/dev/null; then
  echo "Authenticated as permanent admin (restart detected)."
  AUTHENTICATED=true
fi

if [ "$AUTHENTICATED" = true ]; then
  # Check if permanent admin already exists
  if $KCADM get users -r master -q username=admin --fields id 2>/dev/null | grep -q '"id"'; then
    echo "Permanent admin already exists, skipping creation."
  else
    # Create permanent admin user
    $KCADM create users -r master \
      -s username=admin \
      -s email=admin@keycloak.local \
      -s enabled=true \
      -s emailVerified=true

    # Set password
    $KCADM set-password -r master \
      --username admin \
      --new-password admin

    # Assign admin role
    $KCADM add-roles -r master \
      --uname admin \
      --rolename admin

    echo "Permanent admin account created (admin / admin)."
  fi

  # Delete the temporary bootstrap admin (if it still exists)
  TEMP_ID=$($KCADM get users -r master -q "username=$KC_BOOTSTRAP_ADMIN_USERNAME" --fields id 2>/dev/null | grep -o '"id" *: *"[^"]*"' | head -1 | grep -o '[0-9a-f-]\{36\}')
  if [ -n "$TEMP_ID" ] && [ "$KC_BOOTSTRAP_ADMIN_USERNAME" != "admin" ]; then
    $KCADM delete "users/$TEMP_ID" -r master
    echo "Temporary admin '$KC_BOOTSTRAP_ADMIN_USERNAME' deleted."
  fi
else
  echo "WARNING: Could not authenticate to Keycloak admin API."
  echo "  If this is a fresh start, the bootstrap admin may not be ready yet."
  echo "  Keycloak will continue running — configure admin manually at http://localhost:9090"
fi

# Wait for Keycloak process
wait $KC_PID
