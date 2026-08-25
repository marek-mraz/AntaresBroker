*** Settings ***
Documentation       Check that one can delete a batch of entities on the Context Source thanks to a redirect registration

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entities And Registration And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server

*** Variables ***
${entity_payload_filename}              vehicle-simple-different-attributes.jsonld
${entity_pattern}                       urn:ngsi-ld:Vehicle:*
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-batch-ops.jsonld

*** Test Cases ***
D015_01_red Delete Batch Of Entities On Context Source
    [Documentation]    Check that if one requests the Context Broker to delete a batch of entities that match a redirect registration, they are deleted on the Context Source
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-redirect    4_3_6_3    5_6_10

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entityOperations/delete    204
    Set Stub Reply    POST    /broker2/ngsi-ld/v1/entityOperations/delete    204
    ${response}=    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${stub_count}=    Get Stub Count    POST    /broker1/ngsi-ld/v1/entityOperations/delete
    Should Be Equal As Integers    ${stub_count}    1
    ${stub_count}=    Get Stub Count    POST    /broker2/ngsi-ld/v1/entityOperations/delete
    Should Be Equal As Integers    ${stub_count}    1

*** Keywords ***
Create Entities And Registration And Start Context Source Mock Server
    # 5.9.2.4: a redirect registration is refused with Conflict if ANY existing entity
    # matches it; this registration matches urn:ngsi-ld:Vehicle:* so residual Vehicles
    # from earlier tests would legally 409 it. Start from a clean local slate.
    Purge Entities    type=Vehicle    local=true    context=${ngsild_test_suite_context}
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${second_entity_id}

    @{entities_ids_to_be_deleted}=    Create List    ${first_entity_id}    ${second_entity_id}
    Set Suite Variable    ${entities_ids_to_be_deleted}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ...    mode=redirect
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ...    mode=redirect
    ...    endpoint=/broker2
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
    Stop Context Source Mock Server
