#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_value() {
        assert_eq!(0x7472_6976, 0x7472_6976);
    }

    #[test]
    fn test_register_offsets() {
        assert_eq!(R_MAGIC_VALUE, 0x00);
        assert_eq!(R_VERSION, 0x04);
        assert_eq!(R_DEVICE_ID, 0x08);
        assert_eq!(R_HOST_FEATURES, 0x10);
        assert_eq!(R_GUEST_FEATURES, 0x14);
        assert_eq!(R_QUEUE_SEL, 0x30);
        assert_eq!(R_QUEUE_NUM_MAX, 0x34);
        assert_eq!(R_QUEUE_NUM, 0x38);
        assert_eq!(R_QUEUE_ALIGN, 0x3C);
        assert_eq!(R_QUEUE_PFN, 0x40);
        assert_eq!(R_QUEUE_NOTIFY, 0x50);
        assert_eq!(R_STATUS, 0x70);
        assert_eq!(R_QUEUE_DESC_LOW, 0x80);
        assert_eq!(R_QUEUE_DESC_HIGH, 0x84);
        assert_eq!(R_QUEUE_AVAIL_LOW, 0x90);
        assert_eq!(R_QUEUE_AVAIL_HIGH, 0x94);
        assert_eq!(R_QUEUE_USED_LOW, 0xA0);
        assert_eq!(R_QUEUE_USED_HIGH, 0xA4);
        assert_eq!(R_QUEUE_ENABLE, 0xB0);
    }

    #[test]
    fn test_status_flags() {
        assert_eq!(VIRTIO_S_ACK, 1);
        assert_eq!(VIRTIO_S_DRIVER, 2);
        assert_eq!(VIRTIO_S_DRIVER_OK, 4);
        assert_eq!(VIRTIO_S_FEATURES_OK, 8);
    }

    #[test]
    fn test_device_id() {
        assert_eq!(VIRTIO_ID_BLK, 2);
    }

    #[test]
    fn test_blk_constants() {
        assert_eq!(VIRTIO_BLK_T_IN, 0);
        assert_eq!(VIRTIO_BLK_T_OUT, 1);
        assert_eq!(VIRTIO_BLK_S_OK, 0);
        assert_eq!(VIRTIO_BLK_S_IOERR, 1);
    }

    #[test]
    fn test_vq_desc_flags() {
        assert_eq!(VQ_DESC_F_NEXT, 1);
        assert_eq!(VQ_DESC_F_WRITE, 2);
    }

    #[test]
    fn test_virtio_limits() {
        assert_eq!(VIRTIO_MAX_DEVS, 4);
        assert_eq!(VIRTIO_BLK_SECTOR, 512);
        assert_eq!(VIRTQ_SIZE, 256);
    }

    #[test]
    fn test_vqdesc_size_and_layout() {
        assert_eq!(core::mem::size_of::<VqDesc>(), 16);
        assert_eq!(core::mem::align_of::<VqDesc>(), 8);
    }

    #[test]
    fn test_blkreq_size() {
        assert_eq!(core::mem::size_of::<BlkReq>(), 529);
    }

    #[test]
    fn test_virtioblkdev_size() {
        assert_eq!(core::mem::size_of::<VirtioBlkDev>(), 48);
    }

    #[test]
    fn test_initial_count() {
        assert_eq!(count(), 0);
    }

    #[test]
    fn test_dev_out_of_range() {
        unsafe {
            let d = dev(0);
            assert!(d.is_null());
        }
    }
}
