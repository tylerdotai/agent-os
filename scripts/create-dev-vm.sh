#!/bin/bash
# Create development VM on Proxmox

set -e

VM_NAME="agent-os-dev"
VM_ID="9001"
RAM="8192"
CORES="8"
DISK="32G"
ISO_PATH="/var/lib/vz/template/iso/debian-12-genericarm64.qcow2"

# Check if VM already exists
if qm list | grep -q "$VM_ID"; then
    echo "VM $VM_ID already exists. Destroy first with: qm destroy $VM_ID"
    exit 1
fi

# Create VM
echo "Creating VM $VM_NAME (ID: $VM_ID)..."
qm create $VM_ID \
    --name "$VM_NAME" \
    --memory "$RAM" \
    --cores "$CORES" \
    --net0 virtio,bridge=vmbr0 \
    --ostype l26 \
    --arch aarch64

# Add disk
echo "Adding disk..."
qm set $VM_ID --scsi0 local-lvm:0,discard=on,size=$DISK

# Set boot device
qm set $VM_ID --boot order=scsi0

# Download Debian ARM64 ISO if needed
if [ ! -f "$ISO_PATH" ]; then
    echo "Downloading Debian ARM64 ISO..."
    mkdir -p $(dirname $ISO_PATH)
    curl -L -o "$ISO_PATH" "https://cdimage.debian.org/debian-cd/current/arm64/iso-cd/debian-12.7.0-arm64-netinst.iso"
fi

# Attach ISO
qm set $VM_ID --ide2 local:iso/debian-12.7.0-arm64-netinst.iso,media=cdrom

# Start VM
echo "Starting VM..."
qm start $VM_ID

echo "VM $VM_NAME started. Connect with: qm terminal $VM_ID"
echo "After installation, snapshot with: qm snapshot $VM_ID initial"
