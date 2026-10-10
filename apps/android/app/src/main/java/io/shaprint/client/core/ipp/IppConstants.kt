package io.shaprint.client.core.ipp

/**
 * Standard IPP operations, status codes, and attribute tags per RFC 8010 & RFC 8011.
 */
object IppConstants {
    const val IPP_VERSION_2_0: Short = 0x0200

    // Operations
    const val OP_PRINT_JOB: Short = 0x0002
    const val OP_VALIDATE_JOB: Short = 0x0004
    const val OP_CANCEL_JOB: Short = 0x0008
    const val OP_GET_PRINTER_ATTRIBUTES: Short = 0x000B

    // Status Codes
    const val STATUS_SUCCESSFUL_OK: Short = 0x0000
    const val STATUS_CLIENT_ERROR_NOT_FOUND: Short = 0x0406
    const val STATUS_CLIENT_ERROR_ATTRIBUTES_NOT_SUPPORTED: Short = 0x040B

    // Tag Delimiters
    const val TAG_OPERATION_ATTRIBUTES: Byte = 0x01
    const val TAG_JOB_ATTRIBUTES: Byte = 0x02
    const val TAG_END_OF_ATTRIBUTES: Byte = 0x03
    const val TAG_PRINTER_ATTRIBUTES: Byte = 0x04

    // Value Tags
    const val TAG_INTEGER: Byte = 0x21
    const val TAG_BOOLEAN: Byte = 0x22
    const val TAG_ENUM: Byte = 0x23
    const val TAG_OCTET_STRING: Byte = 0x30
    const val TAG_TEXT_WITHOUT_LANGUAGE: Byte = 0x41
    const val TAG_NAME_WITHOUT_LANGUAGE: Byte = 0x42
    const val TAG_KEYWORD: Byte = 0x44
    const val TAG_URI: Byte = 0x45
    const val TAG_CHARSET: Byte = 0x47
    const val TAG_NATURAL_LANGUAGE: Byte = 0x48
    const val TAG_MIME_MEDIA_TYPE: Byte = 0x49

    // Attribute Names
    const val ATTR_ATTRIBUTES_CHARSET = "attributes-charset"
    const val ATTR_ATTRIBUTES_NATURAL_LANGUAGE = "attributes-natural-language"
    const val ATTR_PRINTER_URI = "printer-uri"
    const val ATTR_REQUESTING_USER_NAME = "requesting-user-name"
    const val ATTR_DOCUMENT_FORMAT = "document-format"
    const val ATTR_NETWORK_CHANNEL = "network-channel"
    const val ATTR_COPIES = "copies"
    const val ATTR_SIDES = "sides"
    const val ATTR_MEDIA = "media"
    const val ATTR_PRINT_COLOR_MODE = "print-color-mode"
}
