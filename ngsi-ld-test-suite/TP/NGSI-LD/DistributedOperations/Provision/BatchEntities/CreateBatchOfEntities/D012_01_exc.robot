*** Settings ***
Documentation       Check that one can create a batch of entities on both Context Source and Context Broker thanks to a exclusive registration

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${entity_speed_payload_filename}        vehicle-speed-attribute.jsonld
${entity_pattern}                       urn:ngsi-ld:Vehicle:*
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-batch-ops.jsonld


*** Test Cases ***
D012_01_exc Batch Create Entities With Exclusive Registration
    [Documentation]    Check that if one requests the Context Broker to create a batch of entities that match an exclusive registration, these are created on the Context Source too
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_7

    ${first_speed_entity}=    Load Entity    ${entity_speed_payload_filename}    ${first_entity_id}
    ${second_speed_entity}=    Load Entity    ${entity_speed_payload_filename}    ${second_entity_id}
    @{entities_to_be_created}=    Create List    ${first_speed_entity}    ${second_speed_entity}
    @{expected_entities_ids}=    Create List    ${first_entity_id}    ${second_entity_id}
    Set Suite Variable    @{entities_to_be_created}
    Set Suite Variable    @{expected_entities_ids}

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entityOperations/create    201
    ${response}=    Batch Create Entities
    ...    @{entities_to_be_created}
    Check Response Status Code    201    ${response.status_code}

    ${stub_count}=    Get Stub Count    POST    /broker1/ngsi-ld/v1/entityOperations/create
    Should Be Equal As Integers    ${stub_count}    1


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${second_entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${first_entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${entity_payload_filename}    ${second_entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    mode=exclusive
    ...    endpoint=/broker1
    # ETSI tool bug fixed: 4.3.6.3 - "an id pattern or Entity type defining a group of
    # entities is not supported for exclusive registrations"; an exclusive registration
    # shall name entity ids. Register the two concrete ids instead of urn:ngsi-ld:Vehicle:*.
    ${exclusive_entities}=    Evaluate
    ...    [{"id": $first_entity_id, "type": "Vehicle"}, {"id": $second_entity_id, "type": "Vehicle"}]
    ${registration_payload}=    Update Value To JSON
    ...    ${registration_payload}
    ...    $.information[0].entities
    ...    ${exclusive_entities}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Batch Delete Entities    entities_ids_to_be_deleted=@{expected_entities_ids}
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
