*** Settings ***
Documentation       Check that one can modify an attribute instance in temporal representation of an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Temporal Entity
Suite Teardown      Delete Intitial Temporal Representation Of Entity


*** Variables ***
${filename}=                vehicle-temporal-representation.jsonld
${fragment_filename}=       vehicle-temporal-modify-attribute-instance-fragment.jsonld
${expectation_filename}=    vehicle-temporal-representation-modify-attribute-instance.jsonld
${attributeId}=             speed


*** Test Cases ***
016_01_01 Modify Attribute Instance In Temporal Representation Of An Entity
    [Documentation]    Check that one can partially update an attribute instance of a temporal representation of an entity
    [Tags]    tea-partial-update    5_6_14
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${instanceId_before_update}=    Set Variable    ${response.json()['speed'][0]['instanceId']}

    ${response2}=    Modify Attribute Instance From Temporal Entity
    ...    ${temporal_entity_representation_id}
    ...    ${attributeId}
    ...    ${instanceId_before_update}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}

    Check Response Status Code    204    ${response2.status_code}
    Check Response Body Is Empty    ${response2}
    ${temporal_entity_expectation_payload}=    Load Test Sample
    ...    temporalEntities/expectations/${expectation_filename}
    ...    ${temporal_entity_representation_id}
    ${response3}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${instanceId_after_update}=    Set Variable    ${response3.json()['speed'][0]['instanceId']}

    Should Be Equal As Strings    ${instanceId_before_update}    ${instanceId_after_update}

    ${temporal_entity_expectation_payload}=    Load Test Sample
    ...    temporalEntities/expectations/${expectation_filename}
    ...    ${temporal_entity_representation_id}
    ${ignored_attributes}=    Create List    instanceId    @context    modifiedAt
    Check Updated Resource Set To
    ...    ${temporal_entity_expectation_payload}
    ...    ${response3.json()}
    ...    ${ignored_attributes}


*** Keywords ***
Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${temporal_entity_representation_id}

    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Intitial Temporal Representation Of Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
