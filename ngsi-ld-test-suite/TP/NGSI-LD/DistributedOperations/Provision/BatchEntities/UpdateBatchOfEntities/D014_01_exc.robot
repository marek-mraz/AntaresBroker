*** Settings ***
Documentation       Verify that, when one has an exclusive registration on a Context Broker with redirectionOps, one is able to update a batch of entities on both Context Broker and Context Source

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
${entity_payload_filename}              vehicle-simple-different-attributes.jsonld
${update_payload_filename}              expectations/vehicle-replace-isParked-attribute.jsonld
${entity_pattern}                       urn:ngsi-ld:Vehicle:*
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-batch-ops.jsonld

*** Test Cases *** 
D014_01_exc Update Batch Entities without noOverwrite Option
    [Documentation]    Check that if one request the Context Broker to update a batch of entities that match a redirect registration, they are updated on the Context Source
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_9

    ${first_update_entity}=    Load Entity    ${update_payload_filename}    ${first_entity_id}
    ${second_update_entity}=    Load Entity    ${update_payload_filename}    ${second_entity_id}
    @{entities_to_be_updated}=    Create List    ${first_update_entity}    ${second_update_entity}
    @{entities_ids_to_be_updated}=    Create List    ${first_entity_id}    ${second_entity_id}

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entityOperations/update    204
    ${response}=    Batch Update Entities    @{entities_to_be_updated}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    POST    /broker1/ngsi-ld/v1/entityOperations/update
    Should Be Equal As Integers    ${stub_count}    1

*** Keywords ***
Setup Registration And Start Context Source Mock Server
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${second_entity_id}

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
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}

    # 5.9.2.4: an exclusive registration is refused with Conflict if an entity with a
    # registered id already carries a registered Attribute — so the local copies must be
    # created AFTER the registration (entity creation carries no such restriction).
    ${response}=    Create Entity    ${entity_payload_filename}    ${first_entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${entity_payload_filename}    ${second_entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    @{entities_ids_to_be_deleted}=    Create List    ${first_entity_id}    ${second_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
