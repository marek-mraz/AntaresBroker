*** Settings ***
Documentation       Check that one can create a batch of entities on both Context Source and Context Broker thanks to a inclusive registration

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
${entity_payload_filename}              vehicle-simple-different-attributes.jsonld
${entity_pattern}                       urn:ngsi-ld:Vehicle:*
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-batch-ops.jsonld

*** Test Cases ***
D012_01_inc Batch Create Entities With Inclusive Registration
    [Documentation]    Check that if one requests the Context Broker to create a batch of entities that match an inclusive registration, these are created on the Context Source too
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_6_7
    
    Set Stub Reply    POST    /ngsi-ld/v1/entityOperations/create    201
    ${response}=    Batch Create Entities
    ...    @{entities_to_be_created}
    Check Response Status Code    201    ${response.status_code}

    ${stub_count}=    Get Stub Count    POST    /ngsi-ld/v1/entityOperations/create
    Should Be Equal As Integers    ${stub_count}    1

    @{ignore_keys}=    Create List    @context    speed
    ${entities_to_be_queried}=    Catenate    SEPARATOR=,    ${first_entity_id}    ${second_entity_id}
    ${response}=    Query Entities
    ...    entity_ids=${entities_to_be_queried}
    ...    entity_types=Vehicle
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Created Resources Set To
    ...    expected_resources=${entities_to_be_created}
    ...    response_body=${response.json()}
    ...    ignored_keys=@{ignore_keys}

*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${second_entity_id}

    ${first_entity}=    Load Entity    ${entity_payload_filename}    ${first_entity_id}
    ${second_entity}=    Load Entity    ${entity_payload_filename}    ${second_entity_id}

    @{entities_to_be_created}=    Create List    ${first_entity}    ${second_entity}
    @{expected_entities_ids}=    Create List    ${first_entity_id}    ${second_entity_id}
    Set Suite Variable    @{entities_to_be_created}
    Set Suite Variable    @{expected_entities_ids}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id_pattern=${entity_pattern}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Batch Delete Entities    entities_ids_to_be_deleted=@{expected_entities_ids}
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
