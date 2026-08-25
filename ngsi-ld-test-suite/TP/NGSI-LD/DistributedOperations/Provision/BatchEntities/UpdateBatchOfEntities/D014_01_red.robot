*** Settings ***
Documentation       Verify that, when one has a redirect registration on a Context Broker with redirectionOps, one is able to update a batch of entities on a Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Registration And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server

*** Variables ***
${update_payload_filename}              expectations/vehicle-replace-isParked-attribute.jsonld
${entity_pattern}                       urn:ngsi-ld:Vehicle:*
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-batch-ops.jsonld

*** Test Cases *** 
D014_01_red Update Batch Entities without noOverwrite Option
    [Documentation]    Check that if one request the Context Broker to update a batch of entities that match a redirect registration, they are updated on the Context Source
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_6_9

    ${first_update_entity}=    Load Entity    ${update_payload_filename}    ${first_entity_id}
    ${second_update_entity}=    Load Entity    ${update_payload_filename}    ${second_entity_id}
    @{entities_to_be_updated}=    Create List    ${first_update_entity}    ${second_update_entity}
    @{entities_ids_to_be_updated}=    Create List    ${first_entity_id}    ${second_entity_id}

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entityOperations/update    204
    Set Stub Reply    POST    /broker2/ngsi-ld/v1/entityOperations/update    204
    ${response}=    Batch Update Entities    @{entities_to_be_updated}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    POST    /broker1/ngsi-ld/v1/entityOperations/update
    Should Be Equal As Integers    ${stub_count}    1
    ${stub_count}=    Get Stub Count    POST    /broker2/ngsi-ld/v1/entityOperations/update
    Should Be Equal As Integers    ${stub_count}    1

*** Keywords ***
Setup Registration And Start Context Source Mock Server
    # 5.9.2.4: a redirect registration is refused with Conflict if ANY existing entity
    # matches it; this registration matches urn:ngsi-ld:Vehicle:* so residual Vehicles
    # from earlier tests would legally 409 it. Start from a clean local slate.
    Purge Entities    type=Vehicle    local=true    context=${ngsild_test_suite_context}
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${second_entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ...    mode=redirect
    ...    endpoint=/broker1
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ...    mode=redirect
    ...    endpoint=/broker2
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
    Stop Context Source Mock Server
