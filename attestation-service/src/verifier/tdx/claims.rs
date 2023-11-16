// Copyright (c) 2023 Alibaba Cloud
//
// SPDX-License-Identifier: Apache-2.0
//

//! This module helps parse all fields inside a TDX Quote and CCEL and
//! serialize them into a JSON. The format will look lile
//! ```json
//! {
//!  "ccel": {
//!    "kernel": "5b7aa6572f649714ff00b6a2b9170516a068fd1a0ba72aa8de27574131d454e6396d3bfa1727d9baf421618a942977fa",
//!    "kernel_parameters": {
//!      "console": "hvc0",
//!      "root": "/dev/vda1",
//!      "rw": null
//!    }
//!  },
//!  "quote": {
//!    "header":{
//!        "version": "0400",
//!        "att_key_type": "0200",
//!        "tee_type": "81000000",
//!        "reserved": "00000000",
//!        "vendor_id": "939a7233f79c4ca9940a0db3957f0607",
//!        "user_data": "d099bfec0a477aa85a605dceabf2b10800000000"
//!    },
//!    "body":{
//!        "mr_config_id": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
//!        "mr_owner": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
//!        "mr_owner_config": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
//!        "mr_td": "705ee9381b8633a9fbe532b52345e8433343d2868959f57889d84ca377c395b689cac1599ccea1b7d420483a9ce5f031",
//!        "mrsigner_seam": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
//!        "report_data": "7c71fe2c86eff65a7cf8dbc22b3275689fd0464a267baced1bf94fc1324656aeb755da3d44d098c0c87382f3a5f85b45c8a28fee1d3bdb38342bf96671501429",
//!        "seam_attributes": "0000000000000000",
//!        "td_attributes": "0100001000000000",
//!        "mr_seam": "2fd279c16164a93dd5bf373d834328d46008c2b693af9ebb865b08b2ced320c9a89b4869a9fab60fbe9d0c5a5363c656",
//!        "tcb_svn": "03000500000000000000000000000000",
//!        "xfam": "e742060000000000"
//!    }
//!  }
//!}
//! ```

use anyhow::*;
use as_types::TeeEvidenceParsedClaim;
use byteorder::{LittleEndian, ReadBytesExt};
use serde_json::{Map, Value};

use super::{
    eventlog::{CcEventLog, MeasuredEntity},
    quote::Quote,
};

macro_rules! parse_claim {
    ($map_name: ident, $key_name: literal, $field: ident) => {
        $map_name.insert($key_name.to_string(), serde_json::Value::Object($field))
    };
    ($map_name: ident, $key_name: literal, $field: expr) => {
        $map_name.insert(
            $key_name.to_string(),
            serde_json::Value::String(hex::encode($field)),
        )
    };
}

pub fn generate_parsed_claim(
    quote: Quote,
    cc_eventlog: Option<CcEventLog>,
) -> Result<TeeEvidenceParsedClaim> {
    let mut quote_map = Map::new();
    let mut quote_body = Map::new();
    let mut quote_header = Map::new();
    // Claims from TD Quote Header.
    parse_claim!(quote_header, "version", quote.header.version);
    parse_claim!(quote_header, "att_key_type", quote.header.att_key_type);
    parse_claim!(quote_header, "tee_type", quote.header.tee_type);
    parse_claim!(quote_header, "reserved", quote.header.reserved);
    parse_claim!(quote_header, "vendor_id", quote.header.vendor_id);
    parse_claim!(quote_header, "user_data", quote.header.user_data);
    // Claims from TD Quote Body. We ignore RTMRs because when verifying the integrity of
    // the eventlog (CCEL), they have already been consumed.
    parse_claim!(quote_body, "tcb_svn", quote.report_body.tcb_svn);
    parse_claim!(quote_body, "mr_seam", quote.report_body.mr_seam);
    parse_claim!(quote_body, "mrsigner_seam", quote.report_body.mrsigner_seam);
    parse_claim!(
        quote_body,
        "seam_attributes",
        quote.report_body.seam_attributes
    );
    parse_claim!(quote_body, "td_attributes", quote.report_body.td_attributes);
    parse_claim!(quote_body, "xfam", quote.report_body.xfam);
    parse_claim!(quote_body, "mr_td", quote.report_body.mr_td);
    parse_claim!(quote_body, "mr_config_id", quote.report_body.mr_config_id);
    parse_claim!(quote_body, "mr_owner", quote.report_body.mr_owner);
    parse_claim!(
        quote_body,
        "mr_owner_config",
        quote.report_body.mr_owner_config
    );
    parse_claim!(quote_body, "report_data", quote.report_body.report_data);

    parse_claim!(quote_map, "header", quote_header);
    parse_claim!(quote_map, "body", quote_body);

    // Claims from CC EventLog.
    let mut ccel_map = Map::new();
    if let Some(ccel) = cc_eventlog {
        parse_ccel(ccel, &mut ccel_map)?;
    } else {
        warn!("parse CC EventLog: CCEL is null");
    }

    let mut claims = Map::new();
    parse_claim!(claims, "quote", quote_map);
    parse_claim!(claims, "ccel", ccel_map);
    log::info!("\nParsed Evidence claims map: \n{:?}\n", &claims);

    Ok(Value::Object(claims) as TeeEvidenceParsedClaim)
}

fn parse_ccel(ccel: CcEventLog, ccel_map: &mut Map<String, Value>) -> Result<()> {
    // Digest of kernel using td-shim
    match ccel.query_digest(MeasuredEntity::TdShimKernel) {
        Some(kernel_digest) => {
            ccel_map.insert(
                "kernel".to_string(),
                serde_json::Value::String(kernel_digest),
            );
        }
        _ => {
            warn!("No td-shim kernel hash in CCEL");
        }
    }

    // Digest of kernel using TDVF
    match ccel.query_digest(MeasuredEntity::TdvfKernel) {
        Some(kernel_digest) => {
            ccel_map.insert(
                "kernel".to_string(),
                serde_json::Value::String(kernel_digest),
            );
        }
        _ => {
            warn!("No tdvf kernel hash in CCEL");
        }
    }

    // Map of Kernel Parameters
    match ccel.query_event_data(MeasuredEntity::TdShimKernelParams) {
        Some(config_info) => {
            let td_shim_platform_config_info =
                TdShimPlatformConfigInfo::try_from(&config_info[..])?;

            let parameters = parse_kernel_parameters(td_shim_platform_config_info.data)?;
            ccel_map.insert(
                "kernel_parameters".to_string(),
                serde_json::Value::Object(parameters),
            );
        }
        _ => {
            warn!("No kernel parameters in CCEL");
        }
    }

    Ok(())
}

/// Kernel Commandline Event inside Eventlog
pub struct TdShimPlatformConfigInfo<'a> {
    pub descriptor: [u8; 16],
    pub info_length: u32,
    pub data: &'a [u8],
}

impl<'a> TryFrom<&'a [u8]> for TdShimPlatformConfigInfo<'a> {
    type Error = anyhow::Error;

    fn try_from(data: &'a [u8]) -> std::result::Result<Self, Self::Error> {
        if data.len() < core::mem::size_of::<[u8; 16]>() + core::mem::size_of::<u32>() {
            bail!("give data slice is too short");
        }

        let descriptor = data[0..core::mem::size_of::<[u8; 16]>()].try_into()?;
        let info_length = (&data[core::mem::size_of::<[u8; 16]>()
            ..core::mem::size_of::<[u8; 16]>() + core::mem::size_of::<u32>()])
            .read_u32::<LittleEndian>()?;
        let data = &data[core::mem::size_of::<[u8; 16]>() + core::mem::size_of::<u32>()
            ..core::mem::size_of::<[u8; 16]>()
                + core::mem::size_of::<u32>()
                + info_length as usize];
        Ok(Self {
            descriptor,
            info_length,
            data,
        })
    }
}

fn parse_kernel_parameters(kernel_parameters: &[u8]) -> Result<Map<String, Value>> {
    let parameters_str = String::from_utf8(kernel_parameters.to_vec())?;
    debug!("kernel parameters: {parameters_str}");

    let parameters = parameters_str
        .split(&[' ', '\n', '\r', '\0'])
        .collect::<Vec<&str>>()
        .iter()
        .filter_map(|item| {
            if item.is_empty() {
                return None;
            }
            let it: Vec<&str> = item.split('=').collect();
            match it.len() {
                1 => Some((it[0].to_owned(), Value::Null)),
                2 => Some((it[0].to_owned(), Value::String(it[1].to_owned()))),
                _ => {
                    warn!("Illegal parameter: {item}");
                    None
                }
            }
        })
        .collect();

    Ok(parameters)
}

#[cfg(test)]
mod tests {
    use assert_json_diff::assert_json_eq;
    use serde_json::{json, to_value, Value};

    use crate::verifier::tdx::{eventlog::CcEventLog, quote::parse_tdx_quote};

    use super::{generate_parsed_claim, parse_kernel_parameters};

    #[test]
    fn parse_tdx_claims() {
        let quote_bin = std::fs::read("../test_data/tdx_quote_4.dat").expect("read quote failed");
        let ccel_bin = std::fs::read("../test_data/CCEL_data").expect("read ccel failed");
        let quote = parse_tdx_quote(&quote_bin).expect("parse quote");
        let ccel = CcEventLog::try_from(ccel_bin).expect("parse ccel");
        let claims = generate_parsed_claim(quote, Some(ccel)).expect("parse claim failed");
        let expected = json!({
            "ccel": {
                "kernel": "5b7aa6572f649714ff00b6a2b9170516a068fd1a0ba72aa8de27574131d454e6396d3bfa1727d9baf421618a942977fa",
                "kernel_parameters": {
                    "console": "hvc0",
                    "root": "/dev/vda1",
                    "rw": null
                }
            },
            "quote": {
                "header":{
                    "version": "0400",
                    "att_key_type": "0200",
                    "tee_type": "81000000",
                    "reserved": "00000000",
                    "vendor_id": "939a7233f79c4ca9940a0db3957f0607",
                    "user_data": "d099bfec0a477aa85a605dceabf2b10800000000"
                },
                "body":{
                    "mr_config_id": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                    "mr_owner": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                    "mr_owner_config": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                    "mr_td": "705ee9381b8633a9fbe532b52345e8433343d2868959f57889d84ca377c395b689cac1599ccea1b7d420483a9ce5f031",
                    "mrsigner_seam": "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
                    "report_data": "7c71fe2c86eff65a7cf8dbc22b3275689fd0464a267baced1bf94fc1324656aeb755da3d44d098c0c87382f3a5f85b45c8a28fee1d3bdb38342bf96671501429",
                    "seam_attributes": "0000000000000000",
                    "td_attributes": "0100001000000000",
                    "mr_seam": "2fd279c16164a93dd5bf373d834328d46008c2b693af9ebb865b08b2ced320c9a89b4869a9fab60fbe9d0c5a5363c656",
                    "tcb_svn": "03000500000000000000000000000000",
                    "xfam": "e742060000000000"
                }
            }
        });

        assert_json_eq!(expected, claims);
    }

    #[test]
    fn test_parse_kernel_parameters() {
        #[derive(Debug)]
        struct TestData<'a> {
            params: &'a [u8],
            fail: bool,
            result: Vec<(String, Value)>,
        }

        let tests = &[
            TestData {
                params: b"",
                fail: false,
                result: vec![],
            },
            // Invalid UTF8 data
            TestData {
                params: b"\xff\xff",
                fail: true,
                result: vec![],
            },
            // Invalid UTF8 data
            TestData {
                params: b"foo=\xff\xff",
                fail: true,
                result: vec![],
            },
            TestData {
                params: b"name_only",
                fail: false,
                result: vec![("name_only".into(), Value::Null)],
            },
            TestData {
                params: b"a=b",
                fail: false,
                result: vec![("a".into(), to_value("b").unwrap())],
            },
            TestData {
                params: b"\ra=b",
                fail: false,
                result: vec![("a".into(), to_value("b").unwrap())],
            },
            TestData {
                params: b"\na=b",
                fail: false,
                result: vec![("a".into(), to_value("b").unwrap())],
            },
            TestData {
                params: b"a=b\nc=d",
                fail: false,
                result: vec![
                    ("a".into(), to_value("b").unwrap()),
                    ("c".into(), to_value("d").unwrap()),
                ],
            },
            TestData {
                params: b"a=b\n\nc=d",
                fail: false,
                result: vec![
                    ("a".into(), to_value("b").unwrap()),
                    ("c".into(), to_value("d").unwrap()),
                ],
            },
            TestData {
                params: b"a=b\rc=d",
                fail: false,
                result: vec![
                    ("a".into(), to_value("b").unwrap()),
                    ("c".into(), to_value("d").unwrap()),
                ],
            },
            TestData {
                params: b"a=b\r\rc=d",
                fail: false,
                result: vec![
                    ("a".into(), to_value("b").unwrap()),
                    ("c".into(), to_value("d").unwrap()),
                ],
            },
            TestData {
                params: b"a=b\rc=d\ne=foo",
                fail: false,
                result: vec![
                    ("a".into(), to_value("b").unwrap()),
                    ("c".into(), to_value("d").unwrap()),
                    ("e".into(), to_value("foo").unwrap()),
                ],
            },
            TestData {
                params: b"a=b\rc=d\nname_only\0e=foo",
                fail: false,
                result: vec![
                    ("a".into(), to_value("b").unwrap()),
                    ("c".into(), to_value("d").unwrap()),
                    ("name_only".into(), Value::Null),
                    ("e".into(), to_value("foo").unwrap()),
                ],
            },
            TestData {
                params: b"foo='bar'",
                fail: false,
                result: vec![("foo".into(), to_value("'bar'").unwrap())],
            },
            TestData {
                params: b"foo=\"bar\"",
                fail: false,
                result: vec![("foo".into(), to_value("\"bar\"").unwrap())],
            },
            // Spaces in parameter values are not supported.
            // XXX: Note carefully the apostrophe values below!
            TestData {
                params: b"params_with_spaces_do_not_work='a b c'",
                fail: false,
                result: vec![
                    ("b".into(), Value::Null),
                    ("c'".into(), Value::Null),
                    (
                        "params_with_spaces_do_not_work".into(),
                        to_value("'a").unwrap(),
                    ),
                ],
            },
            TestData {
                params: b"params_with_spaces_do_not_work=\"a b c\"",
                fail: false,
                result: vec![
                    ("b".into(), Value::Null),
                    ("c\"".into(), Value::Null),
                    (
                        "params_with_spaces_do_not_work".into(),
                        to_value("\"a").unwrap(),
                    ),
                ],
            },
            // Params containing equals in their values are silently dropped
            TestData {
                params: b"a==",
                fail: false,
                result: vec![],
            },
            TestData {
                params: b"a==b",
                fail: false,
                result: vec![],
            },
            TestData {
                params: b"a==b=",
                fail: false,
                result: vec![],
            },
            TestData {
                params: b"a=b=c",
                fail: false,
                result: vec![],
            },
            TestData {
                params: b"a==b=c",
                fail: false,
                result: vec![],
            },
            TestData {
                params: b"module_foo=bar=baz,wibble_setting=2",
                fail: false,
                result: vec![],
            },
            TestData {
                params: b"a=b c== d=e",
                fail: false,
                result: vec![
                    ("a".into(), to_value("b").unwrap()),
                    ("d".into(), to_value("e").unwrap()),
                ],
            },
        ];

        for (i, d) in tests.iter().enumerate() {
            let msg = format!(
                "test[{}]: {:?} (params: {:?})",
                i,
                d,
                String::from_utf8_lossy(&d.params.to_vec()),
            );

            let result = parse_kernel_parameters(d.params);

            let msg = format!("{}: actual result: {:?}", msg, result);

            if std::env::var("DEBUG").is_ok() {
                eprintln!("DEBUG: {}", msg);
            }

            if d.fail {
                assert!(result.is_err(), "{}", msg);
                continue;
            }

            let result = result.unwrap();
            let actual_count = result.len();

            let expected_count = d.result.len();

            let msg = format!("{}: actual_count: {:?}", msg, actual_count);

            assert_eq!(actual_count, expected_count, "{}", msg);

            for expected_kv in &d.result {
                let key = &expected_kv.0;
                let value = &expected_kv.1;

                let value_found = result.get(key);

                let kv_msg = format!("{}: key: {:?}, value: {:?}", msg, key, value);

                assert!(value_found.is_some(), "{}", kv_msg);

                let value_found = value_found.unwrap();

                assert_eq!(value_found, value, "{}", kv_msg);
            }
        }
    }
}
