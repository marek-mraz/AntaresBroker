*** Settings ***
Documentation       Verify that, when one has an inclusive registration on a Context Broker with redirectionOps, one is able to update a batch of entities on a Context Source too

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
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-batch-ops.jsonld

*** Test Cases *** 
D014_01_inc Update Batch Entities without noOverwrite Option
    [Documentation]    Check that if one request the Context Broker to update a batch of entities that match an inclusive registration, they are updated on the Context Source too
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_6_9

    ${first_update_entity}=    Load Entity    ${update_payload_filename}    ${first_entity_id}
    ${second_update_entity}=    Load Entity    ${update_payload_filename}    ${second_entity_id}
    @{entities_to_be_updated}=    Create List    ${first_update_entity}    ${second_update_entity}
    @{entities_ids_to_be_updated}=    Create List    ${first_entity_id}    ${second_entity_id}

    Set Stub Reply    POST    /ngsi-ld/v1/entityOperations/update    204
    ${response}=    Batch Update Entities    @{entities_to_be_updated}
    Check Response Status Code    204    ${response.status_code}

    ${expected_entities_ids}=    Catenate    SEPARATOR=,    @{entities_ids_to_be_updated}
    ${response}=    Query Entities
    ...    entity_ids=${expected_entities_ids}
    ...    entity_types=Vehicle
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    
    Should Not Be Equal    ${entities_to_be_updated}    ${response.json()}

*** Keywords ***
Setup Registration And Start Context Source Mock Server
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${second_entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${first_entity_id}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create Entity    ${entity_payload_filename}    ${second_entity_id}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    @{entities_ids_to_be_deleted}=    Create List    ${first_entity_id}    ${second_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
