*** Settings ***
Documentation       Two-broker PROVISION-via-broker1 edges (Antares extension
...                 IOP TPs): every write goes to B1, the data lives (or
...                 lands) in B2, and the assertion reads B2 DIRECTLY with
...                 local=true — proving the operation really crossed the
...                 broker boundary. Covers 5.6.1 create (inclusive vs
...                 redirect), 5.6.2 update attrs, 5.6.3 append attrs,
...                 5.6.5 delete attr and 5.6.6 delete entity, all through
...                 the 4.3.6 distribution rules.

Resource            ${EXECDIR}/resources/ApiUtils/InteropUtils.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Library             Collections

Test Setup          Setup Interop Ids
Test Teardown       Cleanup Interop Fixtures


*** Variables ***
${b1_url}
${b2_url}
${WRITE_OPS}=       ${{ ["createEntity", "updateEntity", "appendAttrs", "updateAttrs", "deleteAttrs", "deleteEntity", "retrieveEntity", "queryEntity"] }}


*** Test Cases ***
IOP_EXT_UPD_01_01 Inclusive Create Via Broker1 Lands In Both Brokers
    [Documentation]    5.6.1.4: with an inclusive registration supporting
    ...    createEntity, the create is serviced locally AND distributed —
    ...    the entity exists in B1 and in B2 (each checked with local=true).
    [Tags]    iop    iop-ext    5_6_1    4_3_6_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    operations=${WRITE_OPS}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    ${created}=    Create Entity At Broker    ${b1_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    42

IOP_EXT_UPD_01_02 Redirect Create Is Never Stored In Broker1
    [Documentation]    4.3.6.3: "the Context Broker itself is not permitted
    ...    to hold context data about the registered Entities" — the create
    ...    via B1 lands ONLY in B2; B1 local=true 404s, yet the federated
    ...    retrieve through B1 still serves it.
    [Tags]    iop    iop-ext    5_6_1    4_3_6_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect
    ...    operations=${WRITE_OPS}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b1_url}    ${entity}

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    200    ${response.status_code}

IOP_EXT_UPD_01_03 Update Attributes Via Broker1 Changes Broker2
    [Documentation]    5.6.2 through 4.3.6: the entity lives in B2; PATCH
    ...    /entities/{id}/attrs at B1 is forwarded — B2's stored value
    ...    changes.
    [Tags]    iop    iop-ext    5_6_2    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect
    ...    operations=${WRITE_OPS}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${fragment}=    Evaluate    {"speed": {"type": "Property", "value": 77}}
    ${response}=    Patch Entity Attrs Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()['speed']['value']}    77

IOP_EXT_UPD_01_04 Append Attribute Via Broker1 Reaches Broker2
    [Documentation]    5.6.3 through 4.3.6: POST /entities/{id}/attrs at B1
    ...    appends a brand-new Attribute to the entity held in B2.
    [Tags]    iop    iop-ext    5_6_3    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect
    ...    operations=${WRITE_OPS}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}

    ${fragment}=    Evaluate    {"brandName": {"type": "Property", "value": "Mercedes"}}
    ${response}=    Append Entity Attrs Via Broker    ${b1_url}    ${entity_id}    ${fragment}
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.json()}    brandName

IOP_EXT_UPD_01_05 Delete Attribute Via Broker1 Removes It In Broker2
    [Documentation]    5.6.5 through 4.3.6: DELETE
    ...    /entities/{id}/attrs/{attr} at B1 — the Attribute disappears from
    ...    B2's copy while the entity itself survives.
    [Tags]    iop    iop-ext    5_6_5    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect
    ...    operations=${WRITE_OPS}
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": $etype, "speed": {"type": "Property", "value": 42}, "brandName": {"type": "Property", "value": "Mercedes"}}
    Create Entity At Broker    ${b2_url}    ${entity}

    ${response}=    Delete Entity Attr Via Broker    ${b1_url}    ${entity_id}    speed
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.json()}    speed
    Dictionary Should Contain Key    ${response.json()}    brandName

IOP_EXT_UPD_01_06 Delete Entity Via Broker1 Removes It From Broker2
    [Documentation]    5.6.6 through 4.3.6: DELETE /entities/{id} at B1 —
    ...    the entity is gone from B2 (local=true 404) and from the
    ...    federated view.
    [Tags]    iop    iop-ext    5_6_6    since_v1.9.1
    Register Broker As Context Source    ${b1_url}    ${registration_id}    ${b2_url}    ${etype}
    ...    mode=redirect
    ...    operations=${WRITE_OPS}
    ${entity}=    Simple Vehicle Entity    ${entity_id}    ${etype}    42
    Create Entity At Broker    ${b2_url}    ${entity}
    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    200    ${response.status_code}

    ${response}=    Delete Entity Via Broker    ${b1_url}    ${entity_id}
    Should Be True    ${response.status_code} in (204, 207)

    ${response}=    Get Entity Via Broker    ${b2_url}    ${entity_id}    local=true
    Check Response Status Code    404    ${response.status_code}
    ${response}=    Get Entity Via Broker    ${b1_url}    ${entity_id}
    Check Response Status Code    404    ${response.status_code}


*** Keywords ***
Setup Interop Ids
    ${suffix}=    Random Interop Suffix
    Set Test Variable    ${etype}    IopVeh${suffix}
    Set Test Variable    ${entity_id}    urn:ngsi-ld:IopVeh:${suffix}
    Set Test Variable    ${registration_id}    urn:ngsi-ld:ContextSourceRegistration:iopext-${suffix}

Cleanup Interop Fixtures
    Delete Entity Via Broker    ${b1_url}    ${entity_id}
    Delete Entity Via Broker    ${b2_url}    ${entity_id}
    Delete Registration At Broker    ${b1_url}    ${registration_id}
