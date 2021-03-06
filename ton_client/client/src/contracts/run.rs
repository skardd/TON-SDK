/*
* Copyright 2018-2020 TON DEV SOLUTIONS LTD.
*
* Licensed under the SOFTWARE EVALUATION License (the "License"); you may not use
* this file except in compliance with the License.  You may obtain a copy of the
* License at: https://ton.dev/licenses
*
* Unless required by applicable law or agreed to in writing, software
* distributed under the License is distributed on an "AS IS" BASIS,
* WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
* See the License for the specific TON DEV software governing permissions and
* limitations under the License.
*/

use ton_sdk::{Contract, MessageType, AbiContract, FunctionCallSet};
use ton_sdk::json_abi::encode_function_call;
use crate::crypto::keys::{KeyPair, account_decode};
use crate::types::{ApiResult, ApiError, base64_decode};
use ton_types::cells_serialization::BagOfCells;
use ton_block::Message as TvmMessage;

use crate::contracts::{EncodedMessage, EncodedUnsignedMessage};
use crate::client::ClientContext;

#[cfg(feature = "node_interaction")]
use ton_sdk::{Transaction, AbiFunction, Message};
#[cfg(feature = "node_interaction")]
use ton_sdk::NodeClient;
#[cfg(feature = "node_interaction")]
use ton_block::{MsgAddressInt, AccStatusChange};
#[cfg(feature = "node_interaction")]
use ed25519_dalek::Keypair;
#[cfg(feature = "node_interaction")]
use futures::StreamExt;


#[cfg(feature = "fee_calculation")]
use ton_sdk::TransactionFees;
#[cfg(feature = "fee_calculation")]
use crate::types::long_num_to_json_string;

fn bool_false() -> bool { false }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFunctionCallSet {
    pub abi: serde_json::Value,
    pub function_name: String,
    pub header: Option<serde_json::Value>,
    pub input: serde_json::Value,
}

impl Into<FunctionCallSet> for RunFunctionCallSet {
    fn into(self) -> FunctionCallSet {
        FunctionCallSet {
            func: self.function_name.clone(),
            header: self.header.map(|value| value.to_string().to_owned()),
            input: self.input.to_string(),
            abi: self.abi.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParamsOfRun {
    pub address: String,
    #[serde(flatten)]
    pub call_set: RunFunctionCallSet,
    pub key_pair: Option<KeyPair>,
    pub try_index: Option<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParamsOfLocalRun {
    pub address: String,
    pub account: Option<serde_json::Value>,
    #[serde(flatten)]
    pub call_set: RunFunctionCallSet,
    pub key_pair: Option<KeyPair>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParamsOfLocalRunWithMsg {
    pub address: String,
    pub account: Option<serde_json::Value>,
    pub abi: Option<serde_json::Value>,
    pub function_name: Option<String>,
    pub message_base64: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParamsOfEncodeUnsignedRunMessage {
    pub address: String,
    #[serde(flatten)]
    pub call_set: RunFunctionCallSet,
    pub try_index: Option<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParamsOfDecodeRunOutput {
    pub abi: serde_json::Value,
    pub function_name: String,
    pub body_base64: String,
    #[serde(default = "bool_false")]
    pub internal: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamsOfDecodeUnknownRun {
    pub abi: serde_json::Value,
    pub body_base64: String,
    #[serde(default = "bool_false")]
    pub internal: bool,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ResultOfRun {
    pub output: serde_json::Value
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRunFees {
    pub in_msg_fwd_fee: String,
    pub storage_fee: String,
    pub gas_fee: String,
    pub out_msgs_fwd_fee: String,
    pub total_account_fees: String,
    pub total_output: String
}

#[cfg(feature = "fee_calculation")]
impl From<TransactionFees> for LocalRunFees {
    fn from(value: TransactionFees) -> Self {
        LocalRunFees {
            in_msg_fwd_fee: long_num_to_json_string(value.in_msg_fwd_fee),
            storage_fee: long_num_to_json_string(value.storage_fee),
            gas_fee: long_num_to_json_string(value.gas_fee),
            out_msgs_fwd_fee: long_num_to_json_string(value.out_msgs_fwd_fee),
            total_account_fees: long_num_to_json_string(value.total_account_fees),
            total_output: long_num_to_json_string(value.total_output),
        }
    }
 }

#[derive(Serialize, Deserialize)]
pub(crate) struct ResultOfLocalRun {
    pub output: Option<serde_json::Value>,
    pub fees: Option<LocalRunFees>
}

#[derive(Serialize, Deserialize)]
pub struct ResultOfDecodeUnknownRun {
    pub function: String,
    pub output: serde_json::Value
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParamsOfGetRunBody {
    pub abi: serde_json::Value,
    pub function: String,
    pub header: Option<serde_json::Value>,
    pub params: serde_json::Value,
    #[serde(default = "bool_false")]
    pub internal: bool,
    pub key_pair: Option<KeyPair>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResultOfGetRunBody {
    pub body_base64: String,
}

#[cfg(feature = "node_interaction")]
pub(crate) async fn run(context: &mut ClientContext, params: ParamsOfRun) -> ApiResult<ResultOfRun> {
    debug!("-> contracts.run({}, {:?})",
        params.address.clone(),
        params.call_set.clone(),
    );

    let address = account_decode(&params.address)?;
    let key_pair = if let Some(ref keys) = params.key_pair { Some(keys.decode()?) } else { None };

    let client = context.get_client()?;
    debug!("run contract");
    let tr = call_contract(client, address, &params, key_pair.as_ref()).await?;

    process_transaction(client, tr, Some(params.call_set.abi), Some(params.call_set.function_name)).await
}

#[cfg(feature = "node_interaction")]
pub(crate) async fn process_transaction(
    client: &NodeClient,
    transaction: Transaction,
    abi: Option<serde_json::Value>,
    function: Option<String>
) -> ApiResult<ResultOfRun> {
    if let Some(abi) = abi {
        let function = function.ok_or(ApiError::contracts_decode_run_output_failed("No function name provided"))?;

        let abi_contract = AbiContract::load(abi.to_string().as_bytes()).expect("Couldn't parse ABI");
        let abi_function = abi_contract.function(&function).expect("Couldn't find function");

        if  transaction.out_messages_id().len() == 0 || !abi_function.has_output() {
            debug!("out messages missing");
            debug!("transaction: {:?}", transaction);
            check_transaction_status(&transaction)?;
            ok_null()
        } else {
            debug!("load out messages");
            let out_msg = load_out_message(client, &transaction, abi_function).await?;
            let response = out_msg.body().expect("error unwrap out message body").into();

            debug!("decode output");
            let result = Contract::decode_function_response_json(
                abi.to_string().to_owned(),
                function.to_owned(),
                response,
                false)
                .expect("Error decoding result");

            debug!("<-");
            Ok(ResultOfRun {
                output: serde_json::from_str(result.as_str())
                    .map_err(|err| ApiError::contracts_decode_run_output_failed(err))?
            })
        }
    } else {
        debug!("No abi provided");
        debug!("transaction: {:?}", transaction);
        check_transaction_status(&transaction)?;
        ok_null()
    }
}

pub(crate) fn serialize_message(msg: TvmMessage) -> ApiResult<(Vec<u8>, String)> {
    let (msg, id) = ton_sdk::Contract::serialize_message(msg)
        .map_err(|err| ApiError::contracts_cannot_serialize_message(err))?;

    Ok((msg, id.to_string()))
}

pub(crate) fn local_run(context: &mut ClientContext, params: ParamsOfLocalRun, tvm_call: bool) -> ApiResult<ResultOfLocalRun> {
    debug!("-> contracts.run.local({}, {:?})",
        params.address.clone(),
        params.call_set.clone()
    );

    let address = account_decode(&params.address)?;

    let key_pair = params.key_pair.map(|pair| pair.decode()).transpose()?;

    let msg = Contract::construct_call_message_json(
        address,
        params.call_set.clone().into(),
        false,
        key_pair.as_ref(),
        None,
        None)
        .map_err(|err| ApiError::contracts_create_run_message_failed(err))?;

    local_run_msg(
        context,
        ParamsOfLocalRunWithMsg {
            address: params.address,
            account: params.account,
            function_name: Some(params.call_set.function_name),
            abi: Some(params.call_set.abi),
            message_base64: base64::encode(&ton_sdk::Contract::serialize_message(msg.message)
                .map_err(|err| ApiError::contracts_cannot_serialize_message(err))?.0)
        },
        tvm_call
    )
}

pub(crate) fn local_run_msg(context: &mut ClientContext, params: ParamsOfLocalRunWithMsg, tvm_call: bool) -> ApiResult<ResultOfLocalRun> {
    debug!("-> contracts.run.local.msg({}, {}, {})",
        params.address.clone(),
        params.function_name.clone().unwrap_or_default(),
        params.message_base64
    );

    let address = account_decode(&params.address)?;

    let contract = match &params.account {
        // load contract data from node manually
        #[cfg(feature = "node_interaction")]
        None => {
            debug!("load contract");
            let mut runtime = context.take_runtime()?;
            let result = runtime.block_on(load_contract(context, &address));
            context.runtime = Some(runtime);
            result?
        }
        // can't load
        #[cfg(not(feature = "node_interaction"))]
        None => {
            debug!("no account provided");
            let _address = address;
            let _context = context;
            return Err(ApiError::invalid_params("", "No account provided"));
        }

        Some(account) => {
            Contract::from_json(&account.to_string())
                .map_err(|err| ApiError::invalid_params(&account.to_string(), err))?
        }
    };

    let msg = Contract::deserialize_message(
        &base64::decode(&params.message_base64)
            .map_err(|err| ApiError::crypto_invalid_base64(&params.message_base64, err))?)
        .map_err(|err| ApiError::invalid_params(&params.message_base64, err))?;

    let (messages, fees) = if !tvm_call {
    #[cfg(feature = "fee_calculation")]
    {
        let result = contract.local_call(msg)
            .map_err(|err| ApiError::contracts_local_run_failed(err))?;
        (result.messages, Some(LocalRunFees::from(result.fees)))
    }
    #[cfg(not(feature = "fee_calculation"))]
    {
        return Err(ApiError::contracts_local_run_failed("Fee calculation feature disabled"));
    }
    } else {
        let messages = contract.local_call_tvm(msg)
            .map_err(|err| ApiError::contracts_local_run_failed(err))?;

        (messages, None)
    };

    if let Some(abi) = params.abi {
        let abi_contract = AbiContract::load(abi.to_string().as_bytes()).expect("Couldn't parse ABI");
        let function = params.function_name.unwrap_or_default();
        let abi_function = abi_contract.function(&function).expect("Couldn't find function");

        for msg in messages {
            if  msg.msg_type() == MessageType::ExternalOutbound &&
                abi_function.is_my_output_message(
                    msg.body().ok_or(ApiError::contracts_decode_run_output_failed("Message has no body"))?,
                    false)
                        .map_err(|err| ApiError::contracts_decode_run_output_failed(err))?
            {
                let output = Contract::decode_function_response_json(
                    abi.to_string(), function, msg.body().expect("Message has no body"), false)
                        .map_err(|err| ApiError::contracts_decode_run_output_failed(err))?;

                let output: serde_json::Value = serde_json::from_str(&output)
                    .map_err(|err| ApiError::contracts_decode_run_output_failed(err))?;

                return Ok(ResultOfLocalRun { output: Some(output), fees });
            }
        }
    }

    Ok(ResultOfLocalRun {
        output: Some(serde_json::Value::default()),
        fees
    })
}

pub(crate) fn encode_message(context: &mut ClientContext, params: ParamsOfRun) -> ApiResult<EncodedMessage> {
    debug!("-> contracts.run.message({}, {:?})",
        params.address.clone(),
        params.call_set.clone()
    );

    let address = account_decode(&params.address)?;
    let key_pair = if let Some(keys) = params.key_pair { Some(keys.decode()?) } else { None };

    let msg = Contract::construct_call_message_json(
        address,
        params.call_set.into(),
        false,
        key_pair.as_ref(),
        Some(context.get_client()?.timeouts()),
        params.try_index)
        .map_err(|err| ApiError::contracts_create_run_message_failed(err))?;

    let (body, id) = serialize_message(msg.message)?;

    debug!("<-");
    Ok(EncodedMessage {
        message_id: id,
        message_body_base64: base64::encode(&body),
        expire: msg.expire
    })
}

pub(crate) fn encode_unsigned_message(context: &mut ClientContext, params: ParamsOfEncodeUnsignedRunMessage) -> ApiResult<EncodedUnsignedMessage> {
    let encoded = ton_sdk::Contract::get_call_message_bytes_for_signing(
        account_decode(&params.address)?,
        params.call_set.into(),
        Some(context.get_client()?.timeouts()),
        params.try_index
    ).map_err(|err| ApiError::contracts_create_run_message_failed(err))?;
    Ok(EncodedUnsignedMessage {
        unsigned_bytes_base64: base64::encode(&encoded.message),
        bytes_to_sign_base64: base64::encode(&encoded.data_to_sign),
        expire: encoded.expire
    })
}

pub(crate) fn decode_output(_context: &mut ClientContext, params: ParamsOfDecodeRunOutput) -> ApiResult<ResultOfRun> {
    let body = base64_decode(&params.body_base64)?;
    let result = Contract::decode_function_response_from_bytes_json(
        params.abi.to_string().to_owned(),
        params.function_name.to_owned(),
        &body,
        params.internal)
            .map_err(|err| ApiError::contracts_decode_run_output_failed(err))?;
    Ok(ResultOfRun {
        output: serde_json::from_str(result.as_str())
            .map_err(|err| ApiError::contracts_decode_run_output_failed(err))?
    })
}

pub(crate) fn decode_unknown_input(_context: &mut ClientContext, params: ParamsOfDecodeUnknownRun) -> ApiResult<ResultOfDecodeUnknownRun> {
    let body = base64_decode(&params.body_base64)?;
    let result = Contract::decode_unknown_function_call_from_bytes_json(
        params.abi.to_string().to_owned(),
        &body,
        params.internal)
            .map_err(|err|ApiError::contracts_decode_run_input_failed(err))?;
    Ok(ResultOfDecodeUnknownRun {
        function: result.function_name,
        output: serde_json::from_str(result.params.as_str())
            .map_err(|err| ApiError::contracts_decode_run_input_failed(err))?
    })
}

pub(crate) fn decode_unknown_output(_context: &mut ClientContext, params: ParamsOfDecodeUnknownRun) -> ApiResult<ResultOfDecodeUnknownRun> {
    let body = base64_decode(&params.body_base64)?;
    let result = Contract::decode_unknown_function_response_from_bytes_json(
        params.abi.to_string().to_owned(),
        &body,
        params.internal)
            .map_err(|err|ApiError::contracts_decode_run_output_failed(err))?;
    Ok(ResultOfDecodeUnknownRun {
        function: result.function_name,
        output: serde_json::from_str(result.params.as_str())
            .map_err(|err| ApiError::contracts_decode_run_output_failed(err))?
    })
}

pub(crate) fn get_run_body(_context: &mut ClientContext, params: ParamsOfGetRunBody) -> ApiResult<ResultOfGetRunBody> {
    debug!("-> contracts.run.body({})", params.params.to_string());

    let keys = match params.key_pair {
        Some(str_pair) => Some(str_pair.decode()?),
        None => None
    };

    let body = encode_function_call(
        params.abi.to_string(),
        params.function,
        params.header.map(|value| value.to_string().to_owned()),
        params.params.to_string(),
        params.internal,
        keys.as_ref())
            .map_err(|err| ApiError::contracts_run_body_creation_failed(err))?;

    let mut data = Vec::new();
    let bag = BagOfCells::with_root(&body.into());
    bag.write_to(&mut data, false)
        .map_err(|err| ApiError::contracts_run_body_creation_failed(err))?;

    debug!("<-");
    Ok(ResultOfGetRunBody {
        body_base64: base64::encode(&data)
    })
}

// Internals
#[cfg(feature = "node_interaction")]
fn ok_null() -> ApiResult<ResultOfRun> {
    Ok(ResultOfRun {
        output: serde_json::Value::Null
    })
}

#[cfg(feature = "node_interaction")]
pub(crate) fn check_transaction_status(transaction: &Transaction) -> ApiResult<()> {
    if !transaction.is_aborted() {
        return Ok(());
    }

    let id = transaction.id().to_string();

    if let Some(storage) = &transaction.storage {
        if storage.status_change != AccStatusChange::Unchanged {
            Err(ApiError::storage_phase_failed(id.clone(), &storage.status_change))?;
        }
    }


    if let Some(reason) = &transaction.compute.skipped_reason {
        Err(ApiError::tvm_execution_skipped(id.clone(), &reason))?;
    }

    if transaction.compute.success.is_none() || !transaction.compute.success.unwrap() {
        Err(ApiError::tvm_execution_failed(
            id.clone(), transaction.compute.exit_code.unwrap_or(-1)))?;
    }

    if let Some(action) = &transaction.action {
        if !action.success {
            Err(ApiError::action_phase_failed(
                    id.clone(),
                    action.result_code,
                    action.valid,
                    action.no_funds,
                ))?;
        }
    }


    Err(ApiError::transaction_aborted(id))
}

#[cfg(feature = "node_interaction")]
async fn load_out_message(client: &NodeClient, tr: &Transaction, abi_function: &AbiFunction) -> ApiResult<Message> {
    let stream = tr.load_out_messages(client)
        .map_err(|err| ApiError::contracts_load_messages_failed(err))?;

    futures::pin_mut!(stream);

    while let Some(msg) = stream.next().await {
        let msg = msg.map_err(|err| ApiError::contracts_load_messages_failed(err))?;

        if  msg.msg_type() == MessageType::ExternalOutbound
            && msg.body().is_some()
            && abi_function.is_my_output_message(msg.body().unwrap(), false)
                .map_err(|err| ApiError::contracts_load_messages_failed(err))?
            {
                return Ok(msg);
            }
    }

    Err(ApiError::contracts_load_messages_failed("No external output messages"))
}

#[cfg(feature = "node_interaction")]
async fn load_contract(context: &ClientContext, address: &MsgAddressInt) -> ApiResult<Contract> {
    let client = context.get_client()?;
    Contract::load_wait_deployed(client, address, None)
        .await
        .map_err(|err| crate::types::apierror_from_sdkerror(err, ApiError::contracts_run_failed))
}

#[cfg(feature = "node_interaction")]
async fn call_contract(
    client: &NodeClient,
    address: MsgAddressInt,
    params: &ParamsOfRun,
    key_pair: Option<&Keypair>,
) -> ApiResult<Transaction> {
    Contract::call_json(
        client,
        address,
        params.call_set.clone().into(),
        key_pair)
            .await
            .map_err(|err| crate::types::apierror_from_sdkerror(err, ApiError::contracts_run_failed))
}
