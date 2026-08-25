*** Settings ***
Documentation       Check that one can update the content of a batch of entities on both Context Source and Context Broker thanks to a inclusive registration

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
${old_entity_payload_filename}              vehicle-simple-attributes.jsonld
${new_entity_payload_filename}              vehicle-simple-attributes-second.jsonld
${entity_pattern}                           urn:ngsi-ld:Vehicle:*
${registration_payload_file_path}           csourceRegistrations/context-source-registration-vehicle-batch-ops.jsonld

*** Test Cases ***
D013_02_inc Batch Upsert Entities With Inclusive Registration With Update Flag
    [Documentation]    Check that if one requests the Context Broker to update a batch of entities that match an inclusive registration, these are updated on the Context Source too
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_6_8
    ${response}=    Retrieve Entity    ${first_entity_id}    context=${ngsild_test_suite_context}
    ${old_body}=    Get From Dictionary    ${response.json()}    brandName

    ${new_first_entity}=    Load Entity    ${new_entity_payload_filename}    ${first_entity_id}
    ${new_second_entity}=    Load Entity    ${new_entity_payload_filename}    ${second_entity_id}
    @{entities_to_be_upserted}=    Create List    ${new_first_entity}    ${new_second_entity}

    Set Stub Reply    POST    /ngsi-ld/v1/entityOperations/upsert    204
    ${response}=    Batch Upsert Entities    @{entities_to_be_upserted}    update_option=update
    Check Response Status Code    204    ${response.status_code}

    # Wait For Request must run BEFORE Get Request Url Params: it reads the "current"
    # request, which only Wait For Request sets (HttpCtrl API doc).
    Wait for redirected request
    ${stub}=    Get Request Url Params    options
    Should Contain    ${stub}    update

    @{upserted_entities_ids}=    Create List    ${first_entity_id}    ${second_entity_id}
    ${expected_updated_entities_ids}=    Catenate    SEPARATOR=,    @{upserted_entities_ids}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_updated_entities_ids}
    ...    entity_types=Vehicle
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    
    ${response}=    Retrieve Entity    ${first_entity_id}    context=${ngsild_test_suite_context}
    ${new_body}=    Get From Dictionary    ${response.json()}    brandName
    Should Not Be Equal    ${old_body}    ${new_body}
    Should Contain    ${response.json()}    isParked2

*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${second_entity_id}

    ${old_entity1}=    Create Entity    ${old_entity_payload_filename}    ${first_entity_id}
    ${old_entity2}=    Create Entity    ${old_entity_payload_filename}    ${second_entity_id}

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
    @{entities_ids_to_be_deleted}=    Create List    ${first_entity_id}    ${second_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server