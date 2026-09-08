/* onyxfs_journal_inject.c — inject a synthetic journal transaction into an
 * OnyxFS v2 image for the crash-recovery e2e test.
 *
 * Works on a copy of the partitioned boot.img built by run_qemu.sh
 * (FAT32 @ LBA 2048, OnyxFS @ LBA 10240). Parses the OnyxFS superblock
 * (block 0 of the partition, 4096-byte FS blocks — layout per
 * core/src/formats/onyxfs_fmt/superblock.rs), then writes either:
 *
 *   --committed   commit_start + block_write(payload) + commit_end
 *                 -> journal_recover() must replay the payload into the
 *                    target data block on mount and zero the journal;
 *   --torn        commit_start + block_write(payload), NO commit_end
 *                 -> journal_recover() must discard the transaction and
 *                    leave the target block untouched.
 *
 * The payload is written to a data block near the end of the image that no
 * live file uses, so verification is a pure content match after boot.
 * Verify mode: --check <payload-file> reads back the target block and
 * prints what recovery did (replayed / untouched), for the shell driver.
 *
 * Build: cc -O2 -o onyxfs_journal_inject onyxfs_journal_inject.c
 */
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <stdlib.h>
#include <errno.h>

#define ONYXFS_LBA     10240UL     /* must match kernel's ONYXFS_LBA */
#define SECTOR         512UL
#define BS             4096UL      /* ONYFS_BLOCK_SIZE */

/* Journal entry type tags (core/src/formats/onyxfs_fmt/journal.rs). */
#define J_COMMIT_START 0u
#define J_BLOCK_WRITE  1u
#define J_COMMIT_END   2u

static const char *img_path;
static FILE *img;

static void die(const char *msg)
{
    fprintf(stderr, "onyxfs_journal_inject: %s (%s)\n", msg, strerror(errno));
    exit(2);
}

static void read_block(uint32_t blk, unsigned char *buf)
{
    if (fseek(img, (long)(ONYXFS_LBA * SECTOR + (unsigned long)blk * BS),
              SEEK_SET))
        die("fseek read");
    if (fread(buf, 1, BS, img) != BS)
        die("fread block");
}

static void write_block(uint32_t blk, const unsigned char *buf)
{
    if (fseek(img, (long)(ONYXFS_LBA * SECTOR + (unsigned long)blk * BS),
              SEEK_SET))
        die("fseek write");
    if (fwrite(buf, 1, BS, img) != BS)
        die("fwrite block");
}

static uint32_t le32(const unsigned char *p)
{
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 |
           (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

static void put_le32(unsigned char *p, uint32_t v)
{
    p[0] = v & 0xFF; p[1] = (v >> 8) & 0xFF;
    p[2] = (v >> 16) & 0xFF; p[3] = (v >> 24) & 0xFF;
}

/* Superblock fields we need (offsets per OnyfsSuper::from_bytes). */
struct sb {
    uint32_t total_blocks, journal_start, journal_size, feature_flags;
};

static void parse_sb(struct sb *s)
{
    unsigned char buf[BS];
    read_block(0, buf);
    if (le32(buf) != 0x32594E4F) /* 'ONY2' */
        die("not an OnyxFS v2 image (bad magic)");
    s->total_blocks  = le32(buf + 12);
    s->journal_start = le32(buf + 44);
    s->journal_size  = le32(buf + 48);
    s->feature_flags = le32(buf + 52);
}

/* 300-byte marker payload: text prefix + 0xA5 filler. */
#define PAYLOAD_LEN 300
static void make_payload(unsigned char *p)
{
    memcpy(p, "ONYX-JRNL-RECOVER-E2E-", 22);
    memset(p + 22, 0xA5, PAYLOAD_LEN - 22);
}

static uint32_t g_target; /* remembered across inject/check in the driver */

static void inject(const struct sb *s, int with_commit)
{
    unsigned char e[BS], old[BS];
    unsigned char payload[PAYLOAD_LEN];
    uint32_t target = s->total_blocks - 2;

    if (s->journal_size < 5) {
        fprintf(stderr, "journal too small: %u blocks\n", s->journal_size);
        exit(2);
    }

    make_payload(payload);

    memset(e, 0, BS);
    put_le32(e, J_COMMIT_START);
    put_le32(e + 4, target);
    write_block(s->journal_start + 0, e);

    memset(e, 0, BS);
    put_le32(e, J_BLOCK_WRITE);
    put_le32(e + 4, target);
    memcpy(e + 8, payload, PAYLOAD_LEN);
    write_block(s->journal_start + 1, e);

    if (with_commit) {
        memset(e, 0, BS);
        put_le32(e, J_COMMIT_END);
        put_le32(e + 4, 0);
        write_block(s->journal_start + 2, e);
    }

    /* Sanity: the target block must not already hold the payload. */
    read_block(target, old);
    if (memcmp(old, payload, PAYLOAD_LEN) == 0)
        die("target block unexpectedly already contains the payload");

    printf("injected %s transaction -> block %u (journal %u..%u)\n",
           with_commit ? "COMMITTED" : "TORN", target,
           s->journal_start, s->journal_start + s->journal_size - 1);
    printf("TARGET_BLOCK %u\n", target);
    g_target = target;
}

/* Verify post-boot: the payload must be present iff the transaction was
 * committed, and the journal area must be zeroed either way.
 *
 * NOTE: the kernel's grow-on-mount rewrites the superblock's total_blocks
 * (1109 -> up to 16384 on a 64 MB image), so the target block CANNOT be
 * recomputed as total-2 after boot — the driver passes the block number
 * printed by --inject as --block N. journal_start itself is unchanged by
 * grow-on-mount, so the zero check still scans from there. */
static void check(uint32_t target, uint32_t journal_start)
{
    unsigned char blk[BS], payload[PAYLOAD_LEN];
    unsigned char j[BS];
    int zeroed = 1, replayed;
    uint32_t scan = 6;

    read_block(target, blk);
    make_payload(payload);
    replayed = memcmp(blk, payload, PAYLOAD_LEN) == 0;

    for (uint32_t i = 0; i < scan; i++) {
        size_t k;
        read_block(journal_start + i, j);
        for (k = 0; k < 64; k++) {
            if (j[k] != 0) {
                zeroed = 0;
                break;
            }
        }
        if (!zeroed)
            break;
    }

    printf("target=%u replayed=%d journal_zeroed=%d\n",
           target, replayed, zeroed);
}

int main(int argc, char **argv)
{
    struct sb s;
    uint32_t check_target = 0;

    if (argc < 3) {
        fprintf(stderr,
                "usage: %s <boot.img> --committed|--torn|--check [--block N]\n",
                argv[0]);
        return 2;
    }
    img_path = argv[1];
    img = fopen(img_path, "r+b");
    if (!img)
        die(img_path);
    parse_sb(&s);
    printf("SB: total=%u journal_start=%u journal_size=%u flags=0x%x\n",
           s.total_blocks, s.journal_start, s.journal_size, s.feature_flags);

    if (strcmp(argv[2], "--committed") == 0)
        inject(&s, 1);
    else if (strcmp(argv[2], "--torn") == 0)
        inject(&s, 0);
    else if (strcmp(argv[2], "--check") == 0) {
        /* Post-boot: take the target block from --block (grow-on-mount
         * invalidates the old total-2 computation), fall back to the
         * pre-boot computation for pre-boot use. */
        check_target = s.total_blocks - 2;
        for (int i = 3; i + 1 < argc; i++) {
            if (strcmp(argv[i], "--block") == 0)
                check_target = (uint32_t)strtoul(argv[i + 1], NULL, 10);
        }
        check(check_target, s.journal_start);
    } else
        die("unknown mode");

    if (fclose(img) != 0)
        die("fclose");
    return 0;
}
